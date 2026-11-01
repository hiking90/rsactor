// `main` of a bin sends as `external`.
use rsactor::spawn;

fn main() {
    let _ = spawn::<senders::Child>(senders::Child);
}
