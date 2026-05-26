use miden_protocol::account::AccountId;
fn main() {
    for s in std::env::args().skip(1) {
        match AccountId::from_bech32(&s) {
            Ok((net, id)) => println!("{} → net={:?} hex={}", s, net, id.to_hex()),
            Err(e) => println!("{}: err {}", s, e),
        }
    }
}
