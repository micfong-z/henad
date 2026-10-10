//! Tests of the thread count that a page's query requests.

use crate::requested_threads;

#[test]
fn an_absent_query_asks_for_every_core() {
    assert_eq!(requested_threads("", 14), 14);
    assert_eq!(requested_threads("?debug=1", 14), 14);
}

#[test]
fn threads_one_is_how_a_single_threaded_run_is_asked_for() {
    assert_eq!(requested_threads("?threads=1", 14), 1);
    assert_eq!(requested_threads("?foo=a&threads=4", 14), 4);
}

/// More workers than cores only adds contention, and zero would build no pool at all.
#[test]
fn a_request_is_clamped_to_the_host() {
    assert_eq!(requested_threads("?threads=99", 14), 14);
    assert_eq!(requested_threads("?threads=0", 14), 1);
    assert_eq!(requested_threads("?threads=nonsense", 14), 14);
}
