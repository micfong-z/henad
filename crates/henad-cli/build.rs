//! Stamps the binary with the commit it was built from and a hash of its sources.

fn main() {
    henad_build::stamp_commit();
}
