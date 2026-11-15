//! Statistics (design §10): sample series from result files, type-1 quantiles with
//! censoring, the order-statistic interval and the tail minimum (milestone 4a); the
//! stationary block bootstrap with the Politis–White block length (4b); sides with
//! hierarchical replication, comparison, verdicts and rates (4c).

pub mod binomial;
pub mod blocklen;
pub mod bootstrap;
pub mod compare;
pub mod describe;
pub mod normal;
pub mod parallel;
pub mod quantile;
pub mod rate;
pub mod segment;
pub mod series;
pub mod side;
pub mod verdict;
pub mod xcorr;
