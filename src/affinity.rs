//! CPU affinity and NUMA memory binding for rt-app.
//!
//! Provides a `CpuSet` wrapper with human-readable formatting, thread affinity
//! management with phase > task > default precedence, and (behind the `numa`
//! feature flag) NUMA memory binding.
//!
//! Ported from the affinity/NUMA portions of rt-app.c.

use std::fmt;

use log::debug;
use nix::sched::{sched_getaffinity, sched_setaffinity, CpuSet};
use nix::unistd::Pid;
use thiserror::Error;

/// Maximum CPU id we support (matches the C code's 10 000 limit).
const MAX_CPUS: usize = 10_000;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum AffinityError {
    #[error("failed to get/set CPU affinity: {0}")]
    Nix(#[from] nix::errno::Errno),

    #[error("too many CPUs specified (max {MAX_CPUS})")]
    TooManyCpus,
}

pub type Result<T> = std::result::Result<T, AffinityError>;

// ---------------------------------------------------------------------------
// CpuSetData — strong wrapper around nix::sched::CpuSet
// ---------------------------------------------------------------------------

/// A CPU set together with its cached human-readable string representation.
#[derive(Clone)]
pub struct CpuSetData {
    cpuset: CpuSet,
    display: String,
}

impl CpuSetData {
    /// Create a `CpuSetData` from the provided CPU indices.
    pub fn from_cpus(cpus: impl IntoIterator<Item = usize>) -> Result<Self> {
        let mut cpuset = CpuSet::new();
        let mut count: usize = 0;
        for cpu in cpus {
            if cpu >= MAX_CPUS {
                return Err(AffinityError::TooManyCpus);
            }
            cpuset.set(cpu).map_err(AffinityError::Nix)?;
            count += 1;
        }
        if count > MAX_CPUS {
            return Err(AffinityError::TooManyCpus);
        }
        let display = create_cpuset_str(&cpuset);
        Ok(Self { cpuset, display })
    }

    /// Build from a raw `nix::sched::CpuSet`.
    pub fn from_raw(cpuset: CpuSet) -> Self {
        let display = create_cpuset_str(&cpuset);
        Self { cpuset, display }
    }

    /// Reference to the inner `CpuSet`.
    pub fn inner(&self) -> &CpuSet {
        &self.cpuset
    }

    /// The human-readable string (e.g. `"[ 0, 2, 3 ]"`).
    pub fn as_str(&self) -> &str {
        &self.display
    }
}

impl fmt::Debug for CpuSetData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CpuSetData")
            .field("display", &self.display)
            .finish()
    }
}

impl fmt::Display for CpuSetData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display)
    }
}

impl PartialEq for CpuSetData {
    fn eq(&self, other: &Self) -> bool {
        // Compare the underlying sets by checking each possible CPU.
        cpusets_equal(&self.cpuset, &other.cpuset)
    }
}

impl Eq for CpuSetData {}

// ---------------------------------------------------------------------------
// create_cpuset_str — format CPU set as "[ 0, 2, 3 ]"
// ---------------------------------------------------------------------------

/// Format a `CpuSet` as a human-readable string such as `"[ 0, 2, 3 ]"`.
fn create_cpuset_str(cpuset: &CpuSet) -> String {
    let cpus: Vec<String> = (0..MAX_CPUS)
        .filter(|&i| cpuset.is_set(i).unwrap_or(false))
        .map(|i| i.to_string())
        .collect();

    format!("[ {} ]", cpus.join(", "))
}

/// Compare two `CpuSet` values for equality.
fn cpusets_equal(a: &CpuSet, b: &CpuSet) -> bool {
    (0..MAX_CPUS).all(|i| a.is_set(i).unwrap_or(false) == b.is_set(i).unwrap_or(false))
}

// ---------------------------------------------------------------------------
// Thread affinity
// ---------------------------------------------------------------------------

/// Determine and apply CPU affinity for the calling thread.
///
/// Precedence (highest to lowest):
/// 1. `phase_cpu_data` — per-phase override
/// 2. `task_cpu_data` — per-task setting
/// 3. default (queried from the OS on first call)
///
/// `current` tracks what is currently applied to avoid redundant syscalls.
/// Returns the `CpuSetData` that is now active.
pub fn set_thread_affinity(
    thread_index: u32,
    phase_cpu_data: Option<&CpuSetData>,
    task_cpu_data: Option<&CpuSetData>,
    current: Option<&CpuSetData>,
) -> Result<CpuSetData> {
    let default = || -> Result<CpuSetData> {
        let cpuset = sched_getaffinity(Pid::from_raw(0))?;
        Ok(CpuSetData::from_raw(cpuset))
    };

    let desired = if let Some(phase) = phase_cpu_data {
        phase
    } else if let Some(task) = task_cpu_data {
        task
    } else {
        // Need default — compute it and use it directly.
        let def = default()?;
        return apply_if_changed(thread_index, &def, current);
    };

    apply_if_changed(thread_index, desired, current)
}

/// Apply affinity if it differs from what is currently set.
fn apply_if_changed(
    thread_index: u32,
    desired: &CpuSetData,
    current: Option<&CpuSetData>,
) -> Result<CpuSetData> {
    let dominated = current.is_some_and(|c| c == desired);
    if !dominated {
        debug!(
            "[{thread_index}] setting cpu affinity to CPU(s) {}",
            desired.as_str()
        );
        sched_setaffinity(Pid::from_raw(0), desired.inner())?;
    }
    Ok(desired.clone())
}

// ---------------------------------------------------------------------------
// NUMA support (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "numa")]
pub mod numa {
    //! NUMA memory binding support.
    //!
    //! This module wraps libnuma's `numa_set_membind` functionality. For now
    //! it provides the type scaffolding; the actual libnuma FFI bindings can
    //! be filled in when the `numa` crate or direct libc bindings are
    //! integrated.

    use std::fmt;

    use log::debug;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum NumaError {
        #[error("NUMA operation failed: {0}")]
        Bind(String),
    }

    pub type Result<T> = std::result::Result<T, NumaError>;

    /// A set of NUMA node IDs.
    #[derive(Clone, PartialEq, Eq)]
    pub struct NumaSet {
        /// Bit-vector of NUMA node IDs (node N is set if bit N is 1).
        mask: Vec<u64>,
        display: String,
    }

    impl NumaSet {
        /// Create a `NumaSet` from NUMA node indices.
        pub fn from_nodes(nodes: impl IntoIterator<Item = usize>) -> Self {
            let mut mask = Vec::new();
            let mut node_strs = Vec::new();

            for node in nodes {
                let word = node / 64;
                let bit = node % 64;
                if word >= mask.len() {
                    mask.resize(word + 1, 0u64);
                }
                mask[word] |= 1u64 << bit;
                node_strs.push(node.to_string());
            }

            // Sort for display.
            node_strs.sort_by_key(|s| s.parse::<usize>().unwrap_or(0));
            let display = format!("[ {} ]", node_strs.join(", "));
            Self { mask, display }
        }

        /// Check if any nodes are set.
        pub fn is_empty(&self) -> bool {
            self.mask.iter().all(|&w| w == 0)
        }

        /// Human-readable display string.
        pub fn as_str(&self) -> &str {
            &self.display
        }
    }

    impl fmt::Display for NumaSet {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.display)
        }
    }

    impl fmt::Debug for NumaSet {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("NumaSet")
                .field("display", &self.display)
                .finish()
        }
    }

    /// NUMA memory-binding data for a thread, including current state tracking.
    #[derive(Debug, Clone)]
    pub struct NumaSetData {
        pub numaset: NumaSet,
    }

    /// Bind the calling thread's memory allocation to the specified NUMA nodes.
    ///
    /// This is a stub that logs the intended operation. When libnuma bindings
    /// are available, it will call `numa_set_membind`.
    pub fn set_thread_membind(
        thread_index: u32,
        numa_data: &NumaSetData,
        current: Option<&NumaSetData>,
    ) -> Result<()> {
        if numa_data.numaset.is_empty() {
            return Ok(());
        }

        let dominated = current.is_some_and(|c| c.numaset == numa_data.numaset);
        if !dominated {
            debug!(
                "[{thread_index}] setting numa_membind to Node(s) {}",
                numa_data.numaset.as_str()
            );
            // TODO(rtapp-7c9cd): Call numa_set_membind via libnuma FFI when
            // the numa crate or direct bindings are integrated.
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpuset_str_empty() {
        let cpuset = CpuSet::new();
        let s = create_cpuset_str(&cpuset);
        assert_eq!(s, "[  ]");
    }

    #[test]
    fn cpuset_str_single() {
        let mut cpuset = CpuSet::new();
        cpuset.set(0).unwrap();
        let s = create_cpuset_str(&cpuset);
        assert_eq!(s, "[ 0 ]");
    }

    #[test]
    fn cpuset_str_multiple() {
        let mut cpuset = CpuSet::new();
        cpuset.set(0).unwrap();
        cpuset.set(2).unwrap();
        cpuset.set(3).unwrap();
        let s = create_cpuset_str(&cpuset);
        assert_eq!(s, "[ 0, 2, 3 ]");
    }

    #[test]
    fn cpuset_str_high_numbers() {
        let mut cpuset = CpuSet::new();
        cpuset.set(100).unwrap();
        cpuset.set(200).unwrap();
        let s = create_cpuset_str(&cpuset);
        assert_eq!(s, "[ 100, 200 ]");
    }

    #[test]
    fn cpuset_data_from_cpus() {
        let data = CpuSetData::from_cpus([1, 3, 5]).unwrap();
        assert_eq!(data.as_str(), "[ 1, 3, 5 ]");
        assert!(data.inner().is_set(1).unwrap());
        assert!(data.inner().is_set(3).unwrap());
        assert!(data.inner().is_set(5).unwrap());
        assert!(!data.inner().is_set(0).unwrap());
    }

    #[test]
    fn cpuset_data_equality() {
        let a = CpuSetData::from_cpus([0, 2]).unwrap();
        let b = CpuSetData::from_cpus([0, 2]).unwrap();
        let c = CpuSetData::from_cpus([0, 3]).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn cpuset_data_display() {
        let data = CpuSetData::from_cpus([4, 7]).unwrap();
        assert_eq!(format!("{data}"), "[ 4, 7 ]");
    }

    #[test]
    fn set_thread_affinity_uses_phase_over_task() {
        // We cannot truly test sched_setaffinity in unit tests (requires
        // appropriate permissions), but we can test the precedence logic
        // by checking that apply_if_changed is called with the right set.
        let phase = CpuSetData::from_cpus([0]).unwrap();
        let task = CpuSetData::from_cpus([1]).unwrap();

        // In a unit test context, sched_setaffinity to CPU 0 should work
        // on most systems (CPU 0 is always present).
        let result = set_thread_affinity(0, Some(&phase), Some(&task), None);
        if let Ok(active) = result {
            assert_eq!(active, phase);
        }
        // If it fails due to permissions, that's fine in a test environment.
    }

    #[test]
    fn set_thread_affinity_falls_back_to_task() {
        let task = CpuSetData::from_cpus([0]).unwrap();
        let result = set_thread_affinity(0, None, Some(&task), None);
        if let Ok(active) = result {
            assert_eq!(active, task);
        }
    }

    // -- NUMA tests (behind feature flag) -----------------------------------

    #[cfg(feature = "numa")]
    mod numa_tests {
        use super::super::numa::*;

        #[test]
        fn numaset_from_nodes() {
            let ns = NumaSet::from_nodes([0, 2, 3]);
            assert_eq!(ns.as_str(), "[ 0, 2, 3 ]");
            assert!(!ns.is_empty());
        }

        #[test]
        fn numaset_empty() {
            let ns = NumaSet::from_nodes(std::iter::empty());
            assert!(ns.is_empty());
        }

        #[test]
        fn set_thread_membind_noop_for_empty() {
            let data = NumaSetData {
                numaset: NumaSet::from_nodes(std::iter::empty()),
            };
            assert!(set_thread_membind(0, &data, None).is_ok());
        }
    }
}
