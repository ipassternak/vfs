use std::{
    cmp,
    collections::{BTreeSet, HashMap},
    fmt,
};

const BLOCK_SIZE: usize = 512;
const INITIAL_BLOCKS_COUNT: usize = 1024;
const DOT: &str = ".";
const DOTDOT: &str = "..";
const PATHNAME_SEPARATOR: &str = "/";
const TRAILING_SEPARATOR: char = '/';
const SYMLINK_RESOLVE_LIMIT: usize = 8;

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
    Symlink(String),
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

    fn as_symlink(&self) -> &str {
        match self {
            Self::Symlink(target) => target,
            _ => panic!("error: fd not a symlink"),
        }
    }

    fn is_dir(&self) -> bool {
        match self {
            FileType::Directory(_) => true,
            _ => false,
        }
    }

    fn is_file(&self) -> bool {
        match self {
            FileType::Regular(_) => true,
            _ => false,
        }
    }

    fn is_symlink(&self) -> bool {
        match self {
            FileType::Symlink(_) => true,
            _ => false,
        }
    }
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Regular(_) => write!(f, "regular file"),
            Self::Directory(_) => write!(f, "directory"),
            Self::Symlink(_) => write!(f, "symbolic link"),
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

    fn new_symlink(target: &str) -> Self {
        Self {
            file_type: FileType::Symlink(target.to_string()),
            size: 0,
            links: 1,
            refs: 0,
        }
    }

    fn stat(&self, name: &str) -> Statx {
        let blocks = match &self.file_type {
            FileType::Regular(blocks_refs) => blocks_refs.iter().filter(|&&id| id != 0).count(),
            FileType::Directory(_) => 0,
            FileType::Symlink(_) => 0,
        };
        Statx {
            name: if self.file_type.is_symlink() {
                format!("{} -> {}", name, self.file_type.as_symlink())
            } else {
                name.to_string()
            },
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

    pub fn is_absolute(pathname: &str) -> bool {
        pathname.starts_with(PATHNAME_SEPARATOR)
    }

    pub fn dirname(pathname: &str) -> String {
        let sep_idx = pathname.rfind(PATHNAME_SEPARATOR);
        match sep_idx {
            Some(idx) => {
                if idx == 0 {
                    PATHNAME_SEPARATOR.to_string()
                } else {
                    pathname[..idx].to_string()
                }
            }
            None => DOT.to_string(),
        }
    }

    pub fn basename(pathname: &str) -> String {
        let sep_idx = pathname.rfind(PATHNAME_SEPARATOR);
        match sep_idx {
            Some(idx) => pathname[idx + 1..].to_string(),
            None => pathname.to_string(),
        }
    }

    fn segmentize(pathname: &str, reverse: bool) -> Vec<&str> {
        let mut segments: Vec<_> = pathname
            .trim_matches(TRAILING_SEPARATOR)
            .split(PATHNAME_SEPARATOR)
            .filter(|&seg| !seg.is_empty())
            .collect();
        if reverse {
            segments.reverse();
        }
        segments
    }

    fn root(&self) -> &FileDescriptor {
        &self.fds[0]
    }

    fn resolve(&self, pathname: &str) -> Option<(&FileDescriptor, usize, usize)> {
        let mut fd = if Vfs::is_absolute(pathname) {
            self.root()
        } else {
            &self.fds[self.cwd_id]
        };
        let mut segments = Vfs::segmentize(pathname, true);
        let mut symlink_resolve_count = 0;
        loop {
            let entries = fd.file_type.as_dir();
            let seg = segments.pop().unwrap_or(DOT);
            match entries.get(seg) {
                Some(&next_id) => match self.fds.get(next_id) {
                    Some(next_fd) => {
                        if segments.is_empty() {
                            return match seg {
                                DOTDOT => {
                                    let entries = next_fd.file_type.as_dir();
                                    Some((next_fd, next_id, entries[DOTDOT]))
                                }
                                DOT => Some((next_fd, next_id, entries[DOTDOT])),
                                _ => Some((next_fd, next_id, entries[DOT])),
                            };
                        }
                        match &next_fd.file_type {
                            FileType::Directory(_) => {
                                fd = next_fd;
                            }
                            FileType::Regular(_) => return None,
                            FileType::Symlink(path) => {
                                if symlink_resolve_count >= SYMLINK_RESOLVE_LIMIT {
                                    return None;
                                }
                                symlink_resolve_count += 1;
                                segments.extend(Vfs::segmentize(path, true));
                                if Vfs::is_absolute(&path) {
                                    fd = self.root();
                                }
                            }
                        };
                    }
                    None => return None,
                },
                None => return None,
            }
        }
    }

    pub fn realpath(&self, pathname: &str) -> Option<String> {
        let (mut realpath, mut fd) = if Vfs::is_absolute(pathname) {
            (Vec::new(), self.root())
        } else {
            (Vfs::segmentize(&self.cwd, false), &self.fds[self.cwd_id])
        };
        let mut segments = Vfs::segmentize(pathname, true);
        let mut symlink_resolve_count = 0;
        while let Some(seg) = segments.pop() {
            let entries = fd.file_type.as_dir();
            match seg {
                DOT => {}
                DOTDOT => {
                    realpath.pop();
                    fd = &self.fds[entries[DOTDOT]];
                }
                _ => match entries.get(seg).and_then(|&next_id| self.fds.get(next_id)) {
                    Some(next_fd) => match &next_fd.file_type {
                        FileType::Directory(_) => {
                            realpath.push(seg);
                            fd = next_fd;
                        }
                        FileType::Regular(_) => {
                            if segments.is_empty() {
                                realpath.push(seg);
                            } else {
                                return None;
                            }
                        }
                        FileType::Symlink(path) => {
                            if symlink_resolve_count >= SYMLINK_RESOLVE_LIMIT {
                                return None;
                            }
                            symlink_resolve_count += 1;
                            let symlink_segments = Vfs::segmentize(path, true);
                            if Vfs::is_absolute(&path) {
                                realpath.clear();
                                fd = self.root();
                            }
                            segments.extend(symlink_segments);
                        }
                    },
                    None => return None,
                },
            }
        }
        Some(format!("{}{}", PATHNAME_SEPARATOR, realpath.join(PATHNAME_SEPARATOR)))
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn symlink(&mut self, path: &str, pathname: &str) -> Result<(), String> {
        let basename = Vfs::basename(&pathname);
        let dirname = Vfs::dirname(&pathname);
        match self.resolve(&dirname) {
            Some((fd, id, _)) => {
                if !fd.file_type.is_dir() {
                    return Err(format!(
                        "symlink: cannot create symlink '{}': Not a directory",
                        pathname
                    ));
                }
                let entries = fd.file_type.as_dir();
                if entries.contains_key(&basename) || basename.is_empty() {
                    return Err(format!(
                        "symlink: cannot create symlink '{}': File exists",
                        pathname
                    ));
                }
                let new_id = self.alloc_fd(|_| FileDescriptor::new_symlink(path));
                let fd = &mut self.fds[id];
                let entries = fd.file_type.as_dir_mut();
                entries.insert(basename.to_string(), new_id);
                Ok(())
            }
            None => Err(format!(
                "symlink: cannot create symlink '{}': No such file or directory",
                pathname
            )),
        }
    }

    pub fn cd(&mut self, pathname: &str) -> Result<(), String> {
        let dirname = &format!("{}/{}", pathname, DOT);
        match self.resolve(dirname) {
            Some((fd, id, _)) => {
                if !fd.file_type.is_dir() {
                    return Err(format!("cd: not a directory: {}", pathname));
                }
                self.cwd = self.realpath(dirname).unwrap();
                self.cwd_id = id;
                Ok(())
            }
            None => Err(format!("cd: no such file or directory: {}", pathname)),
        }
    }

    pub fn mkdir(&mut self, pathname: &str) -> Result<(), String> {
        let pathname = pathname.trim_end_matches(TRAILING_SEPARATOR);
        let basename = Vfs::basename(&pathname);
        let dirname = format!("{}/{}", Vfs::dirname(&pathname), DOT);
        match self.resolve(&dirname) {
            Some((fd, parent_id, _)) => {
                if !fd.file_type.is_dir() {
                    return Err(format!(
                        "mkdir: cannot create directory '{}': Not a directory",
                        dirname
                    ));
                }
                let entries = fd.file_type.as_dir();
                if entries.contains_key(&basename) || basename.is_empty() {
                    return Err(format!("mkdir: cannot create '{}': File exists", pathname));
                }
                let new_id = self.alloc_fd(|id| FileDescriptor::new_dir(id, parent_id));
                let fd = &mut self.fds[parent_id];
                let entries = fd.file_type.as_dir_mut();
                entries.insert(basename.to_string(), new_id);
                Ok(())
            }
            None => Err(format!(
                "mkdir: cannot create directory '{}': No such file or directory",
                pathname
            )),
        }
    }

    pub fn rmdir(&mut self, pathname: &str) -> Result<(), String> {
        match self.resolve(pathname) {
            Some((fd, id, parent_id)) => {
                if id == 0 {
                    return Err(format!(
                        "rmdir: cannot remove '{}': Is a root directory",
                        pathname
                    ));
                }
                if !fd.file_type.is_dir() {
                    return Err(format!(
                        "rmdir: failed to remove '{}': Not a directory",
                        pathname
                    ));
                }
                let entries = fd.file_type.as_dir();
                if entries.len() > 2 {
                    return Err(format!(
                        "rmdir: failed to remove '{}': Directory not empty",
                        pathname
                    ));
                }
                let dir = &mut self.fds[parent_id];
                let entries = dir.file_type.as_dir_mut();
                let name = entries
                    .iter()
                    .find(|(_, &i)| i == id)
                    .map(|(name, _)| name.to_string());
                if let Some(name) = name {
                    entries.remove(&name);
                }
                self.free_fd(id);
                if id == self.cwd_id {
                    self.cwd_id = 0;
                }
                Ok(())
            }
            _ => Err(format!(
                "rmdir: cannot rmdir '{}': No such file or directory",
                pathname
            )),
        }
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
                FileType::Symlink(_) => Ok(vec![pathname.to_string()]),
            },
            None => Err(format!(
                "ls: cannot access '{}': No such file or directory",
                pathname
            )),
        }
    }

    fn alloc_fd<F>(&mut self, f: F) -> usize
    where
        F: FnOnce(usize) -> FileDescriptor,
    {
        let (id, incremented) = self.fds_id.next();
        let fd = f(id);
        if incremented {
            self.fds.push(fd);
        } else {
            self.fds[id] = fd;
        }
        id
    }

    pub fn create(&mut self, pathname: &str) -> Result<(), String> {
        let basename = Vfs::basename(&pathname);
        let dirname = format!("{}/{}", Vfs::dirname(&pathname), DOT);
        match self.resolve(&dirname) {
            Some((fd, id, _)) => {
                if !fd.file_type.is_dir() {
                    return Err(format!(
                        "create: cannot create '{}': Not a directory",
                        dirname
                    ));
                }
                let entries = fd.file_type.as_dir();
                if entries.contains_key(&basename) || basename.is_empty() {
                    return Ok(());
                }
                let new_id = self.alloc_fd(|_| FileDescriptor::new_file());
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
        let basename = Vfs::basename(&pn2);
        let dirname = Vfs::dirname(&pn2);
        let r1 = self.resolve(pn1);
        let r2 = self.resolve(&dirname);
        match (r1, r2) {
            (Some((fd1, id1, _)), Some((fd2, id2, _))) => {
                if fd1.file_type.is_dir() {
                    return Err(format!(
                        "link: cannot create link '{}' to '{}': Operation not permitted",
                        pn2, pn1
                    ));
                }
                if !fd2.file_type.is_dir() {
                    return Err(format!(
                        "link: cannot link '{}' to '{}': Not a directory",
                        pn2, pn1
                    ));
                }
                let fd2 = &mut self.fds[id2];
                let entries = fd2.file_type.as_dir_mut();
                if entries.contains_key(&basename) || basename.is_empty() {
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
            FileType::Directory(_) => {}
            FileType::Symlink(_) => {}
        }
        self.fds_id.free(id);
    }

    pub fn unlink(&mut self, pathname: &str) -> Result<(), String> {
        match self.resolve(pathname) {
            Some((fd, id, parent_id)) => {
                if fd.file_type.is_dir() {
                    return Err(format!(
                        "unlink: cannot unlink '{}': Is a directory",
                        pathname
                    ));
                }
                let dir = &mut self.fds[parent_id];
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
                if !fd.file_type.is_file() {
                    return Err(format!(
                        "open: cannot open '{}': Operation not permitted",
                        pathname
                    ));
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
                if !fd.file_type.is_file() {
                    return Err(format!(
                        "truncate: cannot truncate '{}': Operation not permitted",
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
