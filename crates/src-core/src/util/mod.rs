//! App や GameDatabase に依存しない純粋なユーティリティ。
//!
//! スクリプト層 (`event_runtime`) とデータ層 (`db` / `data`) の双方から使うため、
//! どちらにも属さない最下層に置く。
//! Pure helpers with no dependency on `App` / `GameDatabase`.

pub mod number;
pub mod text;
