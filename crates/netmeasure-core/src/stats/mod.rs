//! Statistics (design §10): sample series from result files, type-1 quantiles with
//! censoring, the order-statistic interval and the tail minimum (milestone 4a); the
//! stationary block bootstrap with the Politis–White block length (4b). Replication
//! levels and verdicts follow in 4c.

pub mod binomial;
pub mod blocklen;
pub mod bootstrap;
pub mod describe;
pub mod quantile;
pub mod segment;
pub mod series;
