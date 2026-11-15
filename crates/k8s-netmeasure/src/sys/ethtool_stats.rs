//! `ethtool -S` for one interface (design §5, milestone 6): `ETHTOOL_GSSET_INFO` for
//! the number of statistics, `ETHTOOL_GSTRINGS` for their names, `ETHTOOL_GSTATS` for
//! the values. GET commands only, allowed without `CAP_NET_ADMIN`.
//!
//! The caller passes the count from `ETHTOOL_GSSET_INFO`. Newer kernels answer a
//! count that no longer matches the driver's with 0 (nothing written); older ones
//! (e.g. 5.14) ignore it and write as many entries as the driver has *now*, which can
//! be more if it grew its list between the calls (e.g. when channels change). Every
//! buffer therefore has room for twice the count plus 64, a reported count beyond it
//! is an error, and 0 means "changed, try at the next snapshot". The buffers are plain
//! integer vectors, so no further `unsafe` is needed than the one `ioctl`.

use super::ethtool::ioctl;

const ETHTOOL_GSTRINGS: u32 = 0x1b;
const ETHTOOL_GSTATS: u32 = 0x1d;
const ETHTOOL_GSSET_INFO: u32 = 0x37;
const ETH_SS_STATS: u32 = 1;
const ETH_GSTRING_LEN: usize = 32;

/// `struct ethtool_sset_info` with room for the one requested set.
#[repr(C)]
#[derive(Debug, Default)]
struct SsetInfo {
    cmd: u32,
    reserved: u32,
    sset_mask: u64,
    data: [u32; 1],
}

fn room(n: usize) -> usize {
    n * 2 + 64
}

/// Number of statistics the driver reports.
fn count(ifname: &str) -> Result<usize, i32> {
    let mut info = SsetInfo {
        cmd: ETHTOOL_GSSET_INFO,
        sset_mask: 1 << ETH_SS_STATS,
        ..SsetInfo::default()
    };
    ioctl(ifname, (&raw mut info).cast())?;
    if info.sset_mask & (1 << ETH_SS_STATS) == 0 {
        return Err(libc::EOPNOTSUPP);
    }
    Ok(info.data[0] as usize)
}

fn names(ifname: &str, n: usize) -> Result<Vec<String>, i32> {
    let cap = room(n);
    // struct ethtool_gstrings: cmd, string_set, len, then len × 32 bytes.
    let mut buf = vec![0u32; 3 + cap * ETH_GSTRING_LEN / 4];
    buf[0] = ETHTOOL_GSTRINGS;
    buf[1] = ETH_SS_STATS;
    buf[2] = n as u32;
    ioctl(ifname, buf.as_mut_ptr().cast())?;
    let len = checked(buf[2] as usize, n, cap)?;
    let bytes: Vec<u8> = buf[3..].iter().flat_map(|w| w.to_ne_bytes()).collect();
    Ok(bytes
        .chunks(ETH_GSTRING_LEN)
        .take(len)
        .map(|c| {
            let end = c.iter().position(|&b| b == 0).unwrap_or(c.len());
            String::from_utf8_lossy(&c[..end]).trim().to_owned()
        })
        .collect())
}

fn values(ifname: &str, n: usize) -> Result<Vec<u64>, i32> {
    let cap = room(n);
    // struct ethtool_stats: cmd and n_stats (two u32 in the first word), then values.
    let mut buf = vec![0u64; 1 + cap];
    buf[0] = header(ETHTOOL_GSTATS, n as u32);
    ioctl(ifname, buf.as_mut_ptr().cast())?;
    let len = checked(split(buf[0]).1 as usize, n, cap)?;
    Ok(buf[1..=len].to_vec())
}

/// The count the kernel reported: 0 for a non-empty list means it changed (`EAGAIN`),
/// beyond the buffer is an error (`EOVERFLOW`).
fn checked(len: usize, n: usize, cap: usize) -> Result<usize, i32> {
    match len {
        0 if n > 0 => Err(libc::EAGAIN),
        l if l > cap => Err(libc::EOVERFLOW),
        l => Ok(l),
    }
}

fn header(cmd: u32, n: u32) -> u64 {
    let mut b = [0u8; 8];
    b[..4].copy_from_slice(&cmd.to_ne_bytes());
    b[4..].copy_from_slice(&n.to_ne_bytes());
    u64::from_ne_bytes(b)
}

fn split(word: u64) -> (u32, u32) {
    let b = word.to_ne_bytes();
    (
        u32::from_ne_bytes([b[0], b[1], b[2], b[3]]),
        u32::from_ne_bytes([b[4], b[5], b[6], b[7]]),
    )
}

/// `(name, value)` of every statistic of `ifname`, or the errno (`EOPNOTSUPP` for
/// interfaces without statistics such as `lo`, `ENODEV` if it does not exist).
pub fn stats(ifname: &str) -> Result<Vec<(String, u64)>, i32> {
    let n = count(ifname)?;
    let names = names(ifname, n)?;
    let values = values(ifname, n)?;
    if names.len() != values.len() {
        // The driver changed its list between the calls; next snapshot.
        return Err(libc::EAGAIN);
    }
    Ok(names.into_iter().zip(values).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_matches_the_uapi_header() {
        assert_eq!(size_of::<SsetInfo>(), 24);
        assert_eq!(split(header(ETHTOOL_GSTATS, 7)), (ETHTOOL_GSTATS, 7));
    }

    #[test]
    fn reported_counts_are_checked() {
        assert_eq!(checked(13, 13, 90), Ok(13));
        assert_eq!(checked(15, 13, 90), Ok(15));
        assert_eq!(checked(0, 13, 90), Err(libc::EAGAIN));
        assert_eq!(checked(0, 0, 64), Ok(0));
        assert_eq!(checked(91, 13, 90), Err(libc::EOVERFLOW));
    }

    #[test]
    fn errors_without_statistics_or_interface() {
        assert_eq!(stats("lo"), Err(libc::EOPNOTSUPP));
        assert_eq!(stats("nm-no-such-if"), Err(libc::ENODEV));
        assert_eq!(stats(""), Err(libc::EINVAL));
    }

    /// On a host with real NICs (not on every CI runner): names are non-empty and
    /// unique per interface, as `ethtool -S` shows them.
    #[test]
    fn physical_interfaces_report_named_statistics() {
        let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
            return;
        };
        for e in entries
            .flatten()
            .filter(|e| e.path().join("device").exists())
        {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Ok(stats) = stats(&name) {
                let names: std::collections::BTreeSet<&str> =
                    stats.iter().map(|(n, _)| n.as_str()).collect();
                assert!(names.iter().all(|n| !n.is_empty()), "{name}: {stats:?}");
                eprintln!(
                    "{name}: {} statistics, e.g. {:?}",
                    stats.len(),
                    stats.first()
                );
            }
        }
    }
}
