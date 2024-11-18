use std::{
    cmp,
    collections::{BTreeSet, HashMap},
    fmt,
};

const BLOCK_SIZE: usize = 512;
const INITIAL_BLOCKS_COUNT: usize = 1024;
const PATHNAME_SEPARATOR: char = '/';
const DOT: &str = ".";
const DOTDOT: &str = "..";

#[derive(Debug)]
struct Identity {
    free: BTreeSet<usize>,
    next: usize,
}

impl Identity {
    /// Create a new `Identity` instance.
    ///
    /// # Arguments
    ///
    /// * `preallocate` - The initial number of integers to allocate.
    /// * `min` - The minimum integer to allocate.
    ///
    /// # Returns
    ///
    /// * A new `Identity` instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use vfs::Identity;
    ///
    /// let preallocate = 10;
    /// let min = 1;
    ///
    /// let id = Identity::new(preallocate, min);
    /// ```
    fn new(preallocate: usize, min: usize) -> Self {
        let next = min + preallocate;
        Self {
            free: BTreeSet::from_iter(min..next),
            next,
        }
    }

    fn next(&mut self) -> (usize, bool) {
        if let Some(id) = self.free.pop_first() {
            (id, false)
        } else {
            let id = self.next;
            self.next += 1;
            (id, true)
        }
    }

    fn free(&mut self, id: usize) {
        self.free.insert(id);
    }
}

#[derive(Debug)]
enum FileType {
    Regular(Vec<usize>),
    Directory(HashMap<String, usize>),
}

impl FileType {
    fn as_dir(&self) -> &HashMap<String, usize> {
        match self {
            Self::Directory(entries) => entries,
            _ => panic!("error: fd not a directory"),
        }
    }

    fn as_dir_mut(&mut self) -> &mut HashMap<String, usize> {
        match self {
            Self::Directory(entries) => entries,
            _ => panic!("error: fd not a directory"),
        }
    }

    fn as_file(&self) -> &Vec<usize> {
        match self {
            Self::Regular(blocks_refs) => blocks_refs,
            _ => panic!("error: fd not a file"),
        }
    }

    fn as_file_mut(&mut self) -> &mut Vec<usize> {
        match self {
            Self::Regular(blocks_refs) => blocks_refs,
            _ => panic!("error: fd not a file"),
        }
    }

    fn is_dir(&self) -> bool {
        match self {
            FileType::Directory(_) => true,
            _ => false,
        }
    }

    fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Regular(_) => write!(f, "regular file"),
            Self::Directory(_) => write!(f, "directory"),
        }
    }
}

#[derive(Debug)]
pub struct Statx {
    name: String,
    size: usize,
    blocks: usize,
    links: usize,
    refs: usize,
    file_type: String,
}

impl fmt::Display for Statx {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "File: {}\nSize: {} \tBlocks: {} \tLinks: {} \tRefs: {} \t {}",
            self.name, self.size, self.blocks, self.links, self.refs, self.file_type
        )
    }
}

#[derive(Debug)]
struct FileDescriptor {
    file_type: FileType,
    size: usize,
    links: usize,
    refs: usize,
}

impl FileDescriptor {
    fn new_file() -> Self {
        Self {
            file_type: FileType::Regular(Vec::new()),
            size: 0,
            links: 1,
            refs: 0,
        }
    }

    fn new_dir(id: usize, parent_id: usize) -> Self {
        let mut entries = HashMap::new();
        entries.insert(DOT.to_string(), id);
        entries.insert(DOTDOT.to_string(), parent_id);
        Self {
            file_type: FileType::Directory(entries),
            size: 0,
            links: 1,
            refs: 0,
        }
    }

    fn stat(&self, name: &str) -> Statx {
        let blocks = match &self.file_type {
            FileType::Regular(blocks_refs) => blocks_refs.iter().filter(|&&id| id != 0).count(),
            FileType::Directory(_) => 0,
        };
        Statx {
            name: name.to_string(),
            size: self.size,
            blocks,
            links: self.links,
            refs: self.refs,
            file_type: format!("{}", self.file_type),
        }
    }
}

#[derive(Debug)]
pub struct Vfs {
    blocks: Vec<u8>,
    fds: Vec<FileDescriptor>,
    open_fds: HashMap<usize, (usize, usize)>,
    blocks_id: Identity,
    fds_id: Identity,
    open_fds_id: Identity,
    cwd_id: usize,
    cwd: String,
}

impl Vfs {
    pub fn new() -> Self {
        Self {
            blocks: vec![0; BLOCK_SIZE * INITIAL_BLOCKS_COUNT],
            fds: vec![FileDescriptor::new_dir(0, 0)],
            open_fds: HashMap::new(),
            blocks_id: Identity::new(INITIAL_BLOCKS_COUNT - 1, 1),
            fds_id: Identity::new(0, 1),
            open_fds_id: Identity::new(0, 0),
            cwd_id: 0,
            cwd: PATHNAME_SEPARATOR.to_string(),
        }
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn is_absolute_path(pathname: &str) -> bool {
        pathname.starts_with(PATHNAME_SEPARATOR)
    }

    pub fn dirname(pathname: &str) -> String {
        let sep_idx = pathname.rfind(PATHNAME_SEPARATOR);
        match sep_idx {
            Some(idx) => pathname[..idx].to_string(),
            None => pathname.to_string(),
        }
    }

    pub fn basename(pathname: &str) -> String {
        let sep_idx = pathname.rfind(PATHNAME_SEPARATOR);
        match sep_idx {
            Some(idx) => pathname[idx + 1..].to_string(),
            None => pathname.to_string(),
        }
    }

    fn root(&self) -> &FileDescriptor {
        &self.fds[0]
    }

    fn resolve(&self, pathname: &str) -> Option<(&FileDescriptor, usize, usize)> {
        let pathname = pathname.trim();
        let mut fd = self.root();
        let mut id: usize = 0;
        if !Vfs::is_absolute_path(pathname) {
            match &self.fds.get(self.cwd_id) {
                Some(cur) => {
                    fd = cur;
                    id = self.cwd_id;
                }
                None => return None,
            }
        }
        let mut segments: Vec<_> = pathname
            .split(PATHNAME_SEPARATOR)
            .filter(|seg| seg.len() > 0)
            .collect();
        let name = segments.pop().unwrap_or(DOT);
        for seg in segments {
            match &fd.file_type {
                FileType::Directory(entries) => {
                    match entries.get(seg).and_then(|&next_id| {
                        id = next_id;
                        self.fds.get(id)
                    }) {
                        Some(next_fd) => {
                            fd = next_fd;
                        }
                        None => return None,
                    }
                }
                FileType::Regular(_) => return None,
            }
        }
        let entries = fd.file_type.as_dir();
        entries
            .get(name)
            .and_then(|&next_id| self.fds.get(next_id).map(|fd| (fd, next_id, id)))
    }

    pub fn stat(&self, pathname: &str) -> Result<Statx, String> {
        match self.resolve(pathname) {
            Some((fd, _, _)) => Ok(fd.stat(pathname)),
            None => Err(format!(
                "stat: cannot statx '{}': No such file or directory",
                pathname
            )),
        }
    }

    pub fn ls(&self, pathname: &str) -> Result<Vec<String>, String> {
        match self.resolve(pathname) {
            Some((fd, _, _)) => match &fd.file_type {
                FileType::Directory(entries) => {
                    let mut names: Vec<_> = entries.keys().cloned().collect();
                    names.sort_unstable();
                    Ok(names)
                }
                FileType::Regular(_) => Ok(vec![pathname.to_string()]),
            },
            None => Err(format!(
                "ls: cannot access '{}': No such file or directory",
                pathname
            )),
        }
    }

    fn alloc_fd(&mut self, is_file: bool, parent_id: usize) -> usize {
        let (id, incremented) = self.fds_id.next();
        let fd = if is_file {
            FileDescriptor::new_file()
        } else {
            FileDescriptor::new_dir(id, parent_id)
        };
        if incremented {
            self.fds.push(fd);
        } else {
            self.fds[id] = fd;
        }
        id
    }

    pub fn create(&mut self, pathname: &str) -> Result<(), String> {
        let basename = Vfs::basename(pathname);
        if basename.is_empty() {
            return Err("create: a non-empty name is required".into());
        }
        let dirname = if pathname.contains(PATHNAME_SEPARATOR) {
            &Vfs::dirname(pathname)
        } else {
            self.cwd()
        };
        match self.resolve(&dirname) {
            Some((fd, id, _)) => {
                if fd.file_type.is_file() {
                    return Err(format!(
                        "create: cannot create '{}': Not a directory",
                        dirname
                    ));
                }
                let entries = fd.file_type.as_dir();
                if entries.contains_key(&basename) {
                    return Ok(());
                }
                let new_id = self.alloc_fd(true, id);
                let fd = &mut self.fds[id];
                let entries = fd.file_type.as_dir_mut();
                entries.insert(basename.to_string(), new_id);
                Ok(())
            }
            None => Err(format!(
                "create: cannot create '{}': No such file or directory",
                pathname
            )),
        }
    }

    pub fn link(&mut self, pn1: &str, pn2: &str) -> Result<(), String> {
        let basename = Vfs::basename(pn2);
        if basename.is_empty() {
            return Err(format!("link: a non-empty name is required"));
        }
        let dirname = if pn2.contains(PATHNAME_SEPARATOR) {
            &Vfs::dirname(pn2)
        } else {
            self.cwd()
        };
        let r1 = self.resolve(pn1);
        let r2 = self.resolve(dirname);
        match (r1, r2) {
            (Some((fd1, id1, _)), Some((fd2, id2, _))) => {
                if fd1.file_type.is_dir() {
                    return Err(format!(
                        "link: cannot create link '{}' to '{}': Operation not permitted",
                        pn2, pn1
                    ));
                }
                if fd2.file_type.is_file() {
                    return Err(format!(
                        "link: cannot link '{}' to '{}': Not a directory",
                        pn2, pn1
                    ));
                }
                let fd2 = &mut self.fds[id2];
                let entries = fd2.file_type.as_dir_mut();
                if entries.contains_key(&basename) {
                    return Err(format!(
                        "link: cannot link '{}' to '{}': File exists",
                        pn2, pn1
                    ));
                }
                entries.insert(basename.to_string(), id1);
                let fd1 = &mut self.fds[id1];
                fd1.links += 1;
                Ok(())
            }
            _ => Err(format!(
                "link: cannot link '{}' to '{}': No such file or directory",
                pn2, pn1,
            )),
        }
    }

    fn free_fd(&mut self, id: usize) {
        let fd = &self.fds[id];
        if fd.links > 0 || fd.refs > 0 {
            return;
        }
        match &fd.file_type {
            FileType::Regular(blocks_refs) => {
                for id in blocks_refs {
                    self.blocks_id.free(*id);
                }
            }
            FileType::Directory(entries) => {
                let ids: Vec<_> = entries
                    .iter()
                    .filter_map(|(name, id)| {
                        if name == &DOT || name == &DOTDOT {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                    .collect();
                for id in ids {
                    self.free_fd(id);
                }
            }
        }
        self.fds_id.free(id);
    }

    pub fn unlink(&mut self, pathname: &str) -> Result<(), String> {
        match self.resolve(pathname) {
            Some((fd, id, dir_id)) => {
                if fd.file_type.is_dir() {
                    return Err(format!(
                        "unlink: cannot unlink '{}': Is a directory",
                        pathname
                    ));
                }
                let dir = &mut self.fds[dir_id];
                let entries = dir.file_type.as_dir_mut();
                let name = Vfs::basename(pathname);
                entries.remove(&name);
                let fd = &mut self.fds[id];
                fd.links -= 1;
                self.free_fd(id);
                Ok(())
            }
            _ => Err(format!(
                "unlink: cannot unlink '{}': No such file or directory",
                pathname
            )),
        }
    }

    pub fn open(&mut self, pathname: &str) -> Result<usize, String> {
        match self.resolve(pathname) {
            Some((fd, id, _)) => {
                if fd.file_type.is_dir() {
                    return Err(format!("open: cannot open '{}': Is a directory", pathname));
                }
                let fd = &mut self.fds[id];
                fd.refs += 1;
                let (oid, _) = self.open_fds_id.next();
                self.open_fds.insert(oid, (id, 0));
                Ok(oid)
            }
            None => Err(format!(
                "open: cannot open '{}': No such file or directory",
                pathname
            )),
        }
    }

    pub fn close(&mut self, oid: usize) -> Result<(), String> {
        match self.open_fds.remove(&oid) {
            Some((id, _)) => {
                self.open_fds_id.free(oid);
                let fd = &mut self.fds[id];
                fd.refs -= 1;
                self.free_fd(id);
                Ok(())
            }
            None => Err(format!("close: invalid file descriptor: {}", oid)),
        }
    }

    pub fn seek(&mut self, oid: usize, offset: usize) -> Result<(), String> {
        match self.open_fds.get_mut(&oid) {
            Some((id, cursor)) => {
                let fd = &self.fds[*id];
                if *cursor > fd.size {
                    *cursor = fd.size;
                }
                if offset > fd.size {
                    return Err(format!("seek: invalid offset: {}", offset));
                }
                *cursor = offset;
                Ok(())
            }
            None => Err(format!("seek: invalid file descriptor: {}", oid)),
        }
    }

    pub fn write(&mut self, oid: usize, data: &[u8]) -> Result<usize, String> {
        match self.open_fds.get_mut(&oid) {
            Some((id, cursor)) => {
                let fd = &mut self.fds[*id];
                let blocks_refs = fd.file_type.as_file_mut();
                let mut rest = data;
                while !rest.is_empty() {
                    let i = *cursor / BLOCK_SIZE;
                    let block_ref = match blocks_refs.get(i).copied().unwrap_or(0) {
                        0 => {
                            let (id, incremented) = self.blocks_id.next();
                            if incremented {
                                let new_len = self.blocks.len() + BLOCK_SIZE;
                                self.blocks.resize(new_len + BLOCK_SIZE, 0);
                            }
                            if blocks_refs.len() == i {
                                blocks_refs.push(id);
                            } else {
                                blocks_refs[i] = id;
                            }
                            id
                        }
                        id => id,
                    };
                    let offset = *cursor % BLOCK_SIZE;
                    let n = (BLOCK_SIZE - offset).min(rest.len());
                    let from = block_ref * BLOCK_SIZE + offset;
                    let to = from + n;
                    self.blocks[from..to].copy_from_slice(&rest[..n]);
                    rest = &rest[n..];
                    *cursor += n;
                }
                fd.size = fd.size.max(*cursor);
                Ok(data.len())
            }
            None => Err(format!("write: invalid file descriptor: {}", oid)),
        }
    }

    pub fn read(&mut self, oid: usize, size: usize) -> Result<Vec<u8>, String> {
        match self.open_fds.get_mut(&oid) {
            Some((id, cursor)) => {
                let fd = &self.fds[*id];
                let blocks_refs = fd.file_type.as_file();
                let mut rest = size.min(fd.size - *cursor);
                let mut data = Vec::with_capacity(rest);
                while rest > 0 {
                    let i = *cursor / BLOCK_SIZE;
                    let block_ref = blocks_refs[i];
                    let offset = *cursor % BLOCK_SIZE;
                    let n = (BLOCK_SIZE - offset).min(rest);
                    let from = block_ref * BLOCK_SIZE + offset;
                    let to = from + n;
                    let some = &self.blocks[from..to];
                    data.extend_from_slice(some);
                    rest -= n;
                    *cursor += n;
                }
                Ok(data)
            }
            None => Err(format!("write: invalid file descriptor: {}", oid)),
        }
    }

    pub fn truncate(&mut self, pathname: &str, size: usize) -> Result<(), String> {
        match self.resolve(pathname) {
            Some((fd, id, _)) => {
                if fd.file_type.is_dir() {
                    return Err(format!(
                        "truncate: cannot truncate '{}': Is a directory",
                        pathname
                    ));
                }
                let fd = &mut self.fds[id];
                let blocks_refs = fd.file_type.as_file_mut();
                match size.cmp(&fd.size) {
                    cmp::Ordering::Less => {
                        let i = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
                        for block_id in blocks_refs.drain(i..) {
                            self.blocks_id.free(block_id);
                        }
                        if fd.refs != 0 {
                            for (fid, cursor) in self.open_fds.values_mut() {
                                if fid == &id {
                                    *cursor = (*cursor).min(size);
                                }
                            }
                        }
                    }
                    cmp::Ordering::Greater => {
                        let new_len = (size + BLOCK_SIZE - 1) / BLOCK_SIZE;
                        blocks_refs.resize(new_len, 0);
                        let j = fd.size / BLOCK_SIZE;
                        let block_ref = blocks_refs[j];
                        if block_ref != 0 {
                            let offset = fd.size % BLOCK_SIZE;
                            let n = (BLOCK_SIZE - offset).min(size - fd.size);
                            let from = block_ref * BLOCK_SIZE + offset;
                            let to = from + n;
                            self.blocks[from..to].fill(0);
                        }
                    }
                    cmp::Ordering::Equal => {}
                }
                fd.size = size;
                Ok(())
            }
            None => Err(format!(
                "truncate: cannot truncate '{}': No such file or directory",
                pathname
            )),
        }
    }
}
