//! Cgroup v1 taskgroup management for rt-app.
//!
//! Provides creation/removal of cgroup directories under the cpu controller
//! mount point, and attachment of threads to specific taskgroups.
//!
//! Ported from rt-app_taskgroups.c.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use log::{debug, error};
use nix::unistd::gettid;
use thiserror::Error;

/// Maximum number of taskgroups that can be allocated.
const MAX_TASKGROUPS: usize = 32;

/// Log prefix for taskgroup messages.
const PIN: &str = "[tg]";

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors arising from taskgroup / cgroup operations.
#[derive(Debug, Error)]
pub enum TaskgroupError {
    #[error("{PIN} taskgroup limit exceeded (max {MAX_TASKGROUPS})")]
    LimitExceeded,

    #[error("{PIN} no cgroup cpu controller found in /proc/cgroups")]
    NoCpuController,

    #[error("{PIN} no cgroup cpu controller mount point found in /proc/mounts")]
    NoMountPoint,

    #[error("{PIN} cgroup operation failed for '{path}': {source}")]
    CgroupIo {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{PIN} cannot attach task to taskgroup '{name}'")]
    AttachFailed { name: TaskgroupName },

    #[error("{PIN} invalid cgroup name '{0}'")]
    InvalidName(String),

    #[error("{PIN} controller not initialized")]
    NotInitialized,
}

pub type Result<T> = std::result::Result<T, TaskgroupError>;

// ---------------------------------------------------------------------------
// Strong types
// ---------------------------------------------------------------------------

/// A validated cgroup path name (e.g. "/mygroup/sub").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskgroupName(String);

impl TaskgroupName {
    /// Create a new `TaskgroupName`, rejecting empty or root-only paths.
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let s = name.into();
        let trimmed = s.trim_matches('/');
        if trimmed.is_empty() {
            return Err(TaskgroupError::InvalidName(s));
        }
        // Normalise: ensure leading slash, no trailing slash.
        let normalised = format!("/{trimmed}");
        Ok(Self(normalised))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Iterate over the cumulative path components.
    ///
    /// For "/a/b/c" yields "/a", "/a/b", "/a/b/c".
    fn cumulative_components(&self) -> Vec<String> {
        let trimmed = self.0.trim_start_matches('/');
        let mut result = Vec::new();
        let mut current = String::new();
        for part in trimmed.split('/') {
            current.push('/');
            current.push_str(part);
            result.push(current.clone());
        }
        result
    }
}

impl std::fmt::Display for TaskgroupName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// TaskgroupData
// ---------------------------------------------------------------------------

/// Per-taskgroup data: the name and the offset within the path at which we
/// first created a directory (for cleanup).
#[derive(Debug, Clone)]
pub struct TaskgroupData {
    pub name: TaskgroupName,
    /// Index into `name.cumulative_components()` of the first directory we
    /// actually created (as opposed to one that already existed). `None` if
    /// all directories already existed.
    pub mkdir_offset: Option<usize>,
}

impl TaskgroupData {
    fn new(name: TaskgroupName) -> Self {
        Self {
            name,
            mkdir_offset: None,
        }
    }
}

// ---------------------------------------------------------------------------
// TaskgroupController
// ---------------------------------------------------------------------------

/// Central controller that manages the cgroup mount point and all taskgroups.
pub struct TaskgroupController {
    mount_point: Option<PathBuf>,
    taskgroups: Vec<TaskgroupData>,
}

impl TaskgroupController {
    pub fn new() -> Self {
        Self {
            mount_point: None,
            taskgroups: Vec::new(),
        }
    }

    /// Number of allocated taskgroups.
    pub fn count(&self) -> usize {
        self.taskgroups.len()
    }

    /// Check that the cpu controller is present and enabled, then discover its
    /// mount point. Must be called before any cgroup directory operations.
    pub fn initialize(&mut self) -> Result<()> {
        if self.taskgroups.is_empty() {
            return Ok(());
        }
        self.check_cpu_controller()?;
        self.find_mount_point()?;
        Ok(())
    }

    // -- Taskgroup slot management ------------------------------------------

    /// Allocate a new taskgroup slot. Returns a mutable reference to it.
    pub fn alloc_taskgroup(&mut self, name: TaskgroupName) -> Result<&mut TaskgroupData> {
        if self.taskgroups.len() >= MAX_TASKGROUPS {
            return Err(TaskgroupError::LimitExceeded);
        }
        self.taskgroups.push(TaskgroupData::new(name));
        debug!("{PIN} # taskgroups allocated [{}]", self.taskgroups.len());
        Ok(self.taskgroups.last_mut().unwrap())
    }

    /// Find an existing taskgroup by name.
    pub fn find_taskgroup(&self, name: &TaskgroupName) -> Option<&TaskgroupData> {
        self.taskgroups.iter().find(|tg| tg.name == *name)
    }

    /// Find an existing taskgroup by name (mutable).
    pub fn find_taskgroup_mut(&mut self, name: &TaskgroupName) -> Option<&mut TaskgroupData> {
        self.taskgroups.iter_mut().find(|tg| tg.name == *name)
    }

    // -- Thread attachment --------------------------------------------------

    /// Attach the calling thread to the given taskgroup.
    pub fn set_thread_taskgroup(&self, tg: &TaskgroupData) -> Result<()> {
        self.attach_task(tg.name.as_str())
            .map_err(|_| TaskgroupError::AttachFailed {
                name: tg.name.clone(),
            })
    }

    /// Move the calling thread back to the root cgroup.
    pub fn reset_thread_taskgroup(&self) -> Result<()> {
        if self.taskgroups.is_empty() {
            return Ok(());
        }
        self.attach_task("/")
    }

    // -- Cgroup directory management ----------------------------------------

    /// Create cgroup directories for all allocated taskgroups.
    pub fn add_cgroups(&mut self) -> Result<()> {
        for i in 0..self.taskgroups.len() {
            let name = self.taskgroups[i].name.clone();
            let offset = self.cgroup_mkdir(&name)?;
            self.taskgroups[i].mkdir_offset = offset;
            debug!("{PIN} cgroup [{name}] added");
        }
        Ok(())
    }

    /// Remove cgroup directories for all allocated taskgroups, in reverse
    /// order.
    pub fn remove_cgroups(&mut self) -> Result<()> {
        if self.mount_point.is_none() {
            return Ok(());
        }
        for i in (0..self.taskgroups.len()).rev() {
            let name = self.taskgroups[i].name.clone();
            let offset = self.taskgroups[i].mkdir_offset;
            self.cgroup_rmdir(&name, offset)?;
            debug!("{PIN} cgroup [{name}] removed");
        }
        Ok(())
    }

    // -- Internal helpers ---------------------------------------------------

    fn mount_point(&self) -> Result<&Path> {
        self.mount_point
            .as_deref()
            .ok_or(TaskgroupError::NotInitialized)
    }

    /// Parse `/proc/cgroups` to verify the `cpu` controller is enabled.
    fn check_cpu_controller(&self) -> Result<()> {
        let content =
            fs::read_to_string("/proc/cgroups").map_err(|e| TaskgroupError::CgroupIo {
                path: PathBuf::from("/proc/cgroups"),
                source: e,
            })?;
        check_cpu_controller_in(&content)
    }

    /// Parse `/proc/mounts` to find the cgroup v1 cpu controller mount point.
    fn find_mount_point(&mut self) -> Result<()> {
        let content = fs::read_to_string("/proc/mounts").map_err(|e| TaskgroupError::CgroupIo {
            path: PathBuf::from("/proc/mounts"),
            source: e,
        })?;
        let mp = find_cpu_mount_point_in(&content)?;
        debug!("{PIN} cgroup cpu controller mountpoint [{mp}] found");
        self.mount_point = Some(PathBuf::from(mp));
        Ok(())
    }

    /// Create nested cgroup directories for the given taskgroup name.
    ///
    /// Returns the index (into `cumulative_components`) of the first directory
    /// we actually had to `mkdir`, or `None` if they all existed already.
    fn cgroup_mkdir(&self, name: &TaskgroupName) -> Result<Option<usize>> {
        let mp = self.mount_point()?;
        let components = name.cumulative_components();
        let mut first_created: Option<usize> = None;

        for (idx, component) in components.iter().enumerate() {
            let full_path = mp.join(component.trim_start_matches('/'));
            match fs::create_dir(&full_path) {
                Ok(()) => {
                    if first_created.is_none() {
                        first_created = Some(idx);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    debug!("{PIN} cgroup [{component}] exists, continue ...");
                }
                Err(e) => {
                    error!("{PIN} cgroup [{component}] unhandled error ({e})");
                    return Err(TaskgroupError::CgroupIo {
                        path: full_path,
                        source: e,
                    });
                }
            }
        }
        Ok(first_created)
    }

    /// Remove cgroup directories in reverse order, starting from the deepest
    /// and stopping at (but not removing) the directory at `offset`.
    ///
    /// Gracefully handles ENOTEMPTY, EBUSY, and ENOENT.
    fn cgroup_rmdir(&self, name: &TaskgroupName, offset: Option<usize>) -> Result<()> {
        let offset = match offset {
            Some(o) => o,
            None => return Ok(()),
        };

        let mp = self.mount_point()?;
        let components = name.cumulative_components();

        for idx in (offset..components.len()).rev() {
            let full_path = mp.join(components[idx].trim_start_matches('/'));
            match fs::remove_dir(&full_path) {
                Ok(()) => {}
                Err(e) => {
                    let component = &components[idx];
                    match e.raw_os_error() {
                        Some(libc::ENOTEMPTY) => {
                            debug!("{PIN} cgroup [{component}] not empty, continue ...");
                        }
                        Some(libc::EBUSY) => {
                            debug!("{PIN} cgroup [{component}] is busy, continue ...");
                        }
                        Some(libc::ENOENT) => {
                            debug!("{PIN} cgroup [{component}] doesn't exist, continue ...");
                        }
                        _ => {
                            error!("{PIN} cgroup [{component}] unhandled error ({e})");
                            return Err(TaskgroupError::CgroupIo {
                                path: full_path,
                                source: e,
                            });
                        }
                    }
                    // C code breaks on *any* error (including handled ones).
                    break;
                }
            }
        }
        Ok(())
    }

    /// Write the calling thread's TID to the tasks file of the given cgroup.
    fn attach_task(&self, name: &str) -> Result<()> {
        let mp = self.mount_point()?;
        let tasks_path = if name == "/" {
            mp.join("tasks")
        } else {
            mp.join(name.trim_start_matches('/')).join("tasks")
        };

        let tid = gettid();
        fs::write(&tasks_path, format!("{tid}")).map_err(|e| TaskgroupError::CgroupIo {
            path: tasks_path,
            source: e,
        })
    }
}

impl Default for TaskgroupController {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pure parsing helpers (testable without /proc)
// ---------------------------------------------------------------------------

/// Check whether the `cpu` controller is listed and enabled in `/proc/cgroups`
/// content.
fn check_cpu_controller_in(content: &str) -> Result<()> {
    // Format: subsys_name  hierarchy  num_cgroups  enabled
    // First line is a header starting with '#'.
    for line in content.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let subsys = match fields.next() {
            Some(s) => s,
            None => continue,
        };
        if subsys != "cpu" {
            continue;
        }
        // Skip hierarchy, num_cgroups; read enabled.
        let enabled = fields.nth(2).and_then(|s| s.parse::<u32>().ok());
        if enabled == Some(1) {
            return Ok(());
        }
    }
    Err(TaskgroupError::NoCpuController)
}

/// Extract the mount point of the cgroup v1 cpu controller from
/// `/proc/mounts` content.
fn find_cpu_mount_point_in(content: &str) -> Result<String> {
    // Format: device  mount_point  fs_type  options  dump  pass
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let _device = fields.next();
        let mount_dir = match fields.next() {
            Some(d) => d,
            None => continue,
        };
        let fs_type = match fields.next() {
            Some(t) => t,
            None => continue,
        };
        if fs_type != "cgroup" {
            continue;
        }
        let options = match fields.next() {
            Some(o) => o,
            None => continue,
        };
        // Options are comma-separated; look for "cpu" (but not just "cpuset").
        if options.split(',').any(|opt| opt == "cpu") {
            return Ok(mount_dir.to_owned());
        }
    }
    Err(TaskgroupError::NoMountPoint)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- TaskgroupName tests ------------------------------------------------

    #[test]
    fn taskgroup_name_normalises_slashes() {
        let name = TaskgroupName::new("mygroup/sub").unwrap();
        assert_eq!(name.as_str(), "/mygroup/sub");

        let name = TaskgroupName::new("/mygroup/sub/").unwrap();
        assert_eq!(name.as_str(), "/mygroup/sub");

        let name = TaskgroupName::new("///a///b///").unwrap();
        assert_eq!(name.as_str(), "/a///b");
    }

    #[test]
    fn taskgroup_name_rejects_empty_and_root() {
        assert!(TaskgroupName::new("").is_err());
        assert!(TaskgroupName::new("/").is_err());
        assert!(TaskgroupName::new("///").is_err());
    }

    #[test]
    fn cumulative_components() {
        let name = TaskgroupName::new("/a/b/c").unwrap();
        let components = name.cumulative_components();
        assert_eq!(components, vec!["/a", "/a/b", "/a/b/c"]);

        let name = TaskgroupName::new("single").unwrap();
        let components = name.cumulative_components();
        assert_eq!(components, vec!["/single"]);
    }

    // -- Parsing tests (mock data) ------------------------------------------

    #[test]
    fn check_cpu_controller_present_and_enabled() {
        let content = "\
#subsys_name\thierarchy\tnum_cgroups\tenabled
cpuset\t5\t1\t1
cpu\t6\t72\t1
cpuacct\t6\t72\t1
blkio\t3\t72\t1
memory\t9\t104\t1
";
        assert!(check_cpu_controller_in(content).is_ok());
    }

    #[test]
    fn check_cpu_controller_disabled() {
        let content = "\
#subsys_name\thierarchy\tnum_cgroups\tenabled
cpu\t6\t72\t0
";
        assert!(check_cpu_controller_in(content).is_err());
    }

    #[test]
    fn check_cpu_controller_missing() {
        let content = "\
#subsys_name\thierarchy\tnum_cgroups\tenabled
memory\t9\t104\t1
";
        assert!(check_cpu_controller_in(content).is_err());
    }

    #[test]
    fn find_mount_point_cgroup_v1() {
        let content = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup /sys/fs/cgroup/cpu,cpuacct cgroup rw,nosuid,nodev,noexec,relatime,cpu,cpuacct 0 0
cgroup /sys/fs/cgroup/memory cgroup rw,nosuid,nodev,noexec,relatime,memory 0 0
cgroup /sys/fs/cgroup/cpuset cgroup rw,nosuid,nodev,noexec,relatime,cpuset 0 0
";
        let mp = find_cpu_mount_point_in(content).unwrap();
        assert_eq!(mp, "/sys/fs/cgroup/cpu,cpuacct");
    }

    #[test]
    fn find_mount_point_not_found() {
        let content = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
cgroup /sys/fs/cgroup/memory cgroup rw,nosuid,nodev,noexec,relatime,memory 0 0
";
        assert!(find_cpu_mount_point_in(content).is_err());
    }

    #[test]
    fn find_mount_point_rejects_cpuset_only() {
        // "cpuset" should NOT match "cpu" — we need an exact option match.
        let content = "\
cgroup /sys/fs/cgroup/cpuset cgroup rw,nosuid,nodev,noexec,relatime,cpuset 0 0
";
        assert!(find_cpu_mount_point_in(content).is_err());
    }

    // -- Controller allocation tests ----------------------------------------

    #[test]
    fn alloc_and_find_taskgroup() {
        let mut ctrl = TaskgroupController::new();
        let name = TaskgroupName::new("group_a").unwrap();
        ctrl.alloc_taskgroup(name.clone()).unwrap();
        assert_eq!(ctrl.count(), 1);

        let found = ctrl.find_taskgroup(&name);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, name);
    }

    #[test]
    fn alloc_respects_max_limit() {
        let mut ctrl = TaskgroupController::new();
        for i in 0..MAX_TASKGROUPS {
            let name = TaskgroupName::new(format!("group_{i}")).unwrap();
            ctrl.alloc_taskgroup(name).unwrap();
        }
        let name = TaskgroupName::new("overflow").unwrap();
        assert!(ctrl.alloc_taskgroup(name).is_err());
    }

    #[test]
    fn find_nonexistent_returns_none() {
        let ctrl = TaskgroupController::new();
        let name = TaskgroupName::new("noexist").unwrap();
        assert!(ctrl.find_taskgroup(&name).is_none());
    }

    // -- Cgroup path construction tests -------------------------------------

    #[test]
    fn cgroup_mkdir_constructs_correct_paths() {
        // Use a temp dir as a fake mount point.
        let tmp = tempfile::tempdir().unwrap();
        let mut ctrl = TaskgroupController::new();
        ctrl.mount_point = Some(tmp.path().to_path_buf());

        let name = TaskgroupName::new("/rtapp/group1").unwrap();
        let offset = ctrl.cgroup_mkdir(&name).unwrap();

        // Both /rtapp and /rtapp/group1 should have been created.
        assert!(tmp.path().join("rtapp").is_dir());
        assert!(tmp.path().join("rtapp/group1").is_dir());
        assert_eq!(offset, Some(0));
    }

    #[test]
    fn cgroup_mkdir_offset_tracks_first_created() {
        let tmp = tempfile::tempdir().unwrap();
        // Pre-create the first level.
        fs::create_dir(tmp.path().join("rtapp")).unwrap();

        let mut ctrl = TaskgroupController::new();
        ctrl.mount_point = Some(tmp.path().to_path_buf());

        let name = TaskgroupName::new("/rtapp/group1").unwrap();
        let offset = ctrl.cgroup_mkdir(&name).unwrap();

        // Only group1 was newly created, so offset should be 1.
        assert_eq!(offset, Some(1));
    }

    #[test]
    fn cgroup_mkdir_all_exist_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("rtapp/group1")).unwrap();

        let mut ctrl = TaskgroupController::new();
        ctrl.mount_point = Some(tmp.path().to_path_buf());

        let name = TaskgroupName::new("/rtapp/group1").unwrap();
        let offset = ctrl.cgroup_mkdir(&name).unwrap();
        assert_eq!(offset, None);
    }

    #[test]
    fn cgroup_rmdir_cleans_up_created_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctrl = TaskgroupController::new();
        ctrl.mount_point = Some(tmp.path().to_path_buf());

        let name = TaskgroupName::new("/rtapp/group1").unwrap();
        let offset = ctrl.cgroup_mkdir(&name).unwrap();

        // Both dirs should exist.
        assert!(tmp.path().join("rtapp/group1").is_dir());
        assert!(tmp.path().join("rtapp").is_dir());

        ctrl.cgroup_rmdir(&name, offset).unwrap();

        // Both should be removed since offset was 0.
        assert!(!tmp.path().join("rtapp/group1").exists());
        assert!(!tmp.path().join("rtapp").exists());
    }

    #[test]
    fn cgroup_rmdir_none_offset_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctrl = TaskgroupController::new();
        ctrl.mount_point = Some(tmp.path().to_path_buf());

        let name = TaskgroupName::new("/something").unwrap();
        // offset = None means nothing was created; rmdir should be a no-op.
        assert!(ctrl.cgroup_rmdir(&name, None).is_ok());
    }

    #[test]
    fn add_and_remove_cgroups_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ctrl = TaskgroupController::new();
        ctrl.mount_point = Some(tmp.path().to_path_buf());

        let name1 = TaskgroupName::new("/rtapp/a").unwrap();
        let name2 = TaskgroupName::new("/rtapp/b").unwrap();
        ctrl.alloc_taskgroup(name1).unwrap();
        ctrl.alloc_taskgroup(name2).unwrap();

        ctrl.add_cgroups().unwrap();
        assert!(tmp.path().join("rtapp/a").is_dir());
        assert!(tmp.path().join("rtapp/b").is_dir());

        ctrl.remove_cgroups().unwrap();
        // /rtapp may still exist (ENOTEMPTY during b removal) but both
        // leaf dirs should be gone.
        assert!(!tmp.path().join("rtapp/a").exists());
        assert!(!tmp.path().join("rtapp/b").exists());
    }
}
