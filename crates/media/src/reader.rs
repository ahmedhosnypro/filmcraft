//! Random-access byte readers: open media without loading the whole file.
//!
//! A [`ByteReader`] serves reads at any offset (a native file, a web `Blob` read in chunks, an
//! in-memory buffer). Container openers that only need the index and then single samples (MP4/MOV,
//! Matroska) open from a reader ([`ReaderOpener`]); formats decoded in one go (stills, WAV,
//! standalone compressed audio) fall back to reading the whole file through it.

use std::io;
use std::sync::Arc;

use crate::{MediaError, Opener, Result, SharedSource};

/// Random-access, read-only bytes.
pub trait ByteReader: Send + Sync {
    /// Total length in bytes.
    fn len(&self) -> u64;
    /// Fill `buf` entirely from `offset`; `UnexpectedEof` past the end. Asynchronous readers fail
    /// with `WouldBlock` (and set [`crate::pending`]) while the range is being fetched.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub type SharedReader = Arc<dyn ByteReader>;

fn eof() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "read past end of byte source")
}

/// An in-memory reader.
pub struct MemReader(pub Arc<[u8]>);

impl ByteReader for MemReader {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let a = usize::try_from(offset).map_err(|_| eof())?;
        let src = self.0.get(a..a.checked_add(buf.len()).ok_or_else(eof)?).ok_or_else(eof)?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

/// Files [`FileReader`]s keep open at most, together (#67). Each open media source used to hold
/// its own descriptor, so a project with hundreds of sources hit the soft descriptor limit (256 on
/// macOS, 1024 on Linux) and the rest showed as failed or offline. Past this many, the least
/// recently read file is closed and its reader reopens it on demand.
#[cfg(any(unix, windows))]
pub const MAX_OPEN_FILES: usize = 128;

/// The open files of a set of [`FileReader`]s, least recently used first.
#[cfg(any(unix, windows))]
#[derive(Debug)]
pub(crate) struct FilePool {
    cap: usize,
    state: std::sync::Mutex<PoolState>,
}

#[cfg(any(unix, windows))]
#[derive(Debug, Default)]
struct PoolState {
    next_id: u64,
    open: std::collections::VecDeque<(u64, Arc<std::fs::File>)>,
}

#[cfg(any(unix, windows))]
impl FilePool {
    pub(crate) fn new(cap: usize) -> Self {
        Self { cap: cap.max(1), state: Default::default() }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn next_id(&self) -> u64 {
        let mut s = self.state();
        s.next_id = s.next_id.wrapping_add(1);
        s.next_id
    }

    /// The open file of reader `id`, now the most recently used.
    fn get(&self, id: u64) -> Option<Arc<std::fs::File>> {
        let mut s = self.state();
        let at = s.open.iter().position(|(i, _)| *i == id)?;
        let entry = s.open.remove(at)?;
        let file = entry.1.clone();
        s.open.push_back(entry);
        Some(file)
    }

    /// Keep `file` open for reader `id`, closing the least recently used files past the cap. A
    /// file another thread is reading from stays open until that read ends.
    fn insert(&self, id: u64, file: std::fs::File) -> Arc<std::fs::File> {
        let mut s = self.state();
        if let Some((_, open)) = s.open.iter().find(|(i, _)| *i == id) {
            return open.clone(); // another thread reopened it first
        }
        let file = Arc::new(file);
        s.open.push_back((id, file.clone()));
        while s.open.len() > self.cap {
            s.open.pop_front();
        }
        file
    }

    fn remove(&self, id: u64) {
        self.state().open.retain(|(i, _)| *i != id);
    }

    #[cfg(test)]
    fn open_count(&self) -> usize {
        self.state().open.len()
    }
}

#[cfg(any(unix, windows))]
static FILE_POOL: std::sync::LazyLock<Arc<FilePool>> = std::sync::LazyLock::new(|| Arc::new(FilePool::new(MAX_OPEN_FILES)));

/// A file on disk, read in place: only the requested ranges are read, so opening a clip costs its
/// index rather than its size. The file is kept open in a shared pool of at most
/// [`MAX_OPEN_FILES`] and reopened by path when the pool closed it.
#[cfg(any(unix, windows))]
#[derive(Debug)]
pub struct FileReader {
    path: std::path::PathBuf,
    len: u64,
    id: u64,
    pool: Arc<FilePool>,
}

#[cfg(any(unix, windows))]
impl FileReader {
    pub fn open(path: &std::path::Path) -> io::Result<Self> {
        Self::open_in(path, &FILE_POOL)
    }

    pub(crate) fn open_in(path: &std::path::Path, pool: &Arc<FilePool>) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let m = file.metadata()?;
        if !m.is_file() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} is not a file", path.display())));
        }
        let id = pool.next_id();
        pool.insert(id, file);
        Ok(Self { path: path.to_path_buf(), len: m.len(), id, pool: pool.clone() })
    }

    /// The open file, reopened when the pool closed it. A file now shorter than when it was opened
    /// (replaced or truncated) is refused rather than read as the old one; a longer one (still
    /// being written) is read up to the opened length, as before.
    fn file(&self) -> io::Result<Arc<std::fs::File>> {
        if let Some(file) = self.pool.get(self.id) {
            return Ok(file);
        }
        let file = std::fs::File::open(&self.path)?;
        let m = file.metadata()?;
        if !m.is_file() || m.len() < self.len {
            return Err(io::Error::other(format!("{} changed since it was opened", self.path.display())));
        }
        Ok(self.pool.insert(self.id, file))
    }
}

#[cfg(any(unix, windows))]
impl Drop for FileReader {
    fn drop(&mut self) {
        self.pool.remove(self.id);
    }
}

#[cfg(any(unix, windows))]
impl ByteReader for FileReader {
    fn len(&self) -> u64 {
        self.len
    }
    /// Positional reads: threads decoding different parts of the file do not share a cursor.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let file = self.file()?;
        #[cfg(unix)]
        {
            std::os::unix::fs::FileExt::read_exact_at(&*file, buf, offset)
        }
        #[cfg(windows)]
        {
            let (mut done, mut at) = (0usize, offset);
            while let Some(rest) = buf.get_mut(done..).filter(|r| !r.is_empty()) {
                match std::os::windows::fs::FileExt::seek_read(&*file, rest, at) {
                    Ok(0) => return Err(eof()),
                    Ok(n) => {
                        done = done.saturating_add(n);
                        at = at.saturating_add(n as u64);
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        }
    }
}

/// Read `len` bytes at `offset` (fewer at the end of the reader).
pub fn read_range(r: &dyn ByteReader, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    let n = (r.len().saturating_sub(offset)).min(len as u64) as usize;
    let mut v = vec![0u8; n];
    r.read_at(offset, &mut v)?;
    Ok(v)
}

/// Opens a container from a reader: `head` is the first bytes of the file (for sniffing).
/// Returns `None` when the format is not this opener's.
pub type ReaderOpener = fn(name: &str, head: &[u8], reader: &SharedReader) -> Option<Result<SharedSource>>;

/// Bytes sniffed by [`open_reader`].
pub const HEAD_LEN: usize = 64 * 1024;

/// Open media from a reader: reader openers first; otherwise read the whole file and use the
/// byte openers (`extra`, then stills / WAV).
pub fn open_reader(name: &str, reader: SharedReader, reader_openers: &[ReaderOpener], extra: &[Opener]) -> Result<SharedSource> {
    open_reader_within(name, reader, reader_openers, extra, u64::MAX)
}

/// Like [`open_reader`], but a file no reader opener takes is read whole only up to `max_whole`
/// bytes; a larger one is refused instead. For looking at files rather than importing them (the
/// Media Browser's properties and thumbnails, #157): reading a multi-gigabyte AVI or WAV into
/// memory to show its duration, or to find out it isn't supported, thrashes the disk.
pub fn open_reader_within(name: &str, reader: SharedReader, reader_openers: &[ReaderOpener], extra: &[Opener], max_whole: u64) -> Result<SharedSource> {
    let head = read_range(&*reader, 0, HEAD_LEN).map_err(|e| MediaError::Io(format!("{name}: {e}")))?;
    for o in reader_openers {
        if let Some(r) = o(name, &head, &reader) {
            return r;
        }
    }
    if reader.len() > max_whole {
        return Err(MediaError::Unsupported(format!("{name}: no streaming reader for this format, and it is too large to read whole here")));
    }
    let all = read_range(&*reader, 0, usize::try_from(reader.len()).unwrap_or(usize::MAX)).map_err(|e| MediaError::Io(format!("{name}: {e}")))?;
    crate::open_bytes(name, all.into(), extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_reader_reads_ranges_and_reports_eof() {
        let r = MemReader(Arc::from(&b"0123456789"[..]));
        let mut b = [0u8; 3];
        r.read_at(4, &mut b).unwrap();
        assert_eq!(&b, b"456");
        assert_eq!(r.read_at(8, &mut b).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(read_range(&r, 8, 100).unwrap(), b"89");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn file_reader_reads_ranges_in_place() {
        let dir = std::env::temp_dir().join(format!("filmcraft-file-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bytes.bin");
        std::fs::write(&path, b"0123456789").unwrap();
        let r = FileReader::open(&path).unwrap();
        assert_eq!(r.len(), 10);
        let mut b = [0u8; 3];
        r.read_at(4, &mut b).unwrap();
        assert_eq!(&b, b"456");
        // reads are positional: an earlier offset after a later one
        r.read_at(0, &mut b).unwrap();
        assert_eq!(&b, b"012");
        assert_eq!(r.read_at(8, &mut b).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(read_range(&r, 8, 100).unwrap(), b"89");
        assert!(FileReader::open(&dir).is_err(), "a directory is not a media file");
        assert_eq!(FileReader::open(&dir.join("missing")).unwrap_err().kind(), io::ErrorKind::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// #67: more readers than the pool keeps open stay readable; past the cap the least recently
    /// read file is closed and reopened on demand, and a file truncated meanwhile is refused.
    #[cfg(any(unix, windows))]
    #[test]
    fn more_file_readers_than_the_pool_holds_stay_readable() {
        let dir = std::env::temp_dir().join(format!("filmcraft-file-pool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = Arc::new(FilePool::new(2));
        let paths: Vec<_> = (0..5).map(|i| dir.join(format!("{i}.bin"))).collect();
        for (i, p) in paths.iter().enumerate() {
            std::fs::write(p, format!("file{i}-0123456789")).unwrap();
        }
        let readers: Vec<_> = paths.iter().map(|p| FileReader::open_in(p, &pool).unwrap()).collect();
        assert_eq!(pool.open_count(), 2);
        for _ in 0..2 {
            for (i, r) in readers.iter().enumerate() {
                let mut b = [0u8; 5];
                r.read_at(0, &mut b).unwrap();
                assert_eq!(b, *format!("file{i}").as_bytes());
                assert!(pool.open_count() <= 2);
            }
        }
        // reader 0 was closed by the later reads; its file is now shorter than when opened
        std::fs::write(&paths[0], b"short").unwrap();
        let mut b = [0u8; 5];
        assert!(readers[0].read_at(0, &mut b).is_err());
        drop(readers);
        assert_eq!(pool.open_count(), 0, "dropped readers close their files");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A reader that counts the bytes read from it.
    struct Counting(MemReader, std::sync::atomic::AtomicU64);
    impl ByteReader for Counting {
        fn len(&self) -> u64 {
            self.0.len()
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
            self.1.fetch_add(buf.len() as u64, std::sync::atomic::Ordering::SeqCst);
            self.0.read_at(offset, buf)
        }
    }

    /// #157: looking at a file no streaming reader takes (an AVI, a large WAV) read all of it.
    /// Within a limit it reads the head only and refuses; a small file still opens.
    #[test]
    fn open_within_reads_only_the_head_of_large_unstreamable_files() {
        let mut wav = crate::wav::write_wav16(&vec![0.25; 48_000 * 2], 2, 48_000);
        let small = wav.len() as u64;
        let r = Arc::new(Counting(MemReader(Arc::from(wav.clone())), Default::default()));
        let src = open_reader_within("a.wav", r.clone(), &[], &[], small).unwrap();
        assert_eq!(src.info().audio().unwrap().channels, 2);
        // the same file over the limit: refused after reading the sniffing head only
        let r = Arc::new(Counting(MemReader(Arc::from(wav.clone())), Default::default()));
        let e = open_reader_within("a.wav", r.clone(), &[], &[], small - 1).err().unwrap();
        assert!(matches!(e, MediaError::Unsupported(_)), "{e:?}");
        assert!(r.1.load(std::sync::atomic::Ordering::SeqCst) <= HEAD_LEN as u64);
        // an unsupported format is refused without being read whole either
        wav.resize(4 * HEAD_LEN, 0);
        wav[..4].copy_from_slice(b"RIFF");
        wav[8..12].copy_from_slice(b"AVI ");
        let r = Arc::new(Counting(MemReader(Arc::from(wav)), Default::default()));
        assert!(open_reader_within("a.avi", r.clone(), &[], &[], HEAD_LEN as u64).is_err());
        assert!(r.1.load(std::sync::atomic::Ordering::SeqCst) <= HEAD_LEN as u64);
    }

    #[test]
    fn falls_back_to_byte_openers() {
        // a WAV is opened through the whole-file path
        let wav = crate::wav::write_wav16(&[0.0, 0.5, -0.5, 0.25], 2, 48_000);
        let r: SharedReader = Arc::new(MemReader(wav.into()));
        let s = open_reader("a.wav", r, &[], &[]).unwrap();
        assert!(s.info().has_audio());
    }
}
