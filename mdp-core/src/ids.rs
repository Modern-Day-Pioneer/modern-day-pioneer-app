//! Permanent IDs: a random code of letters and digits, created once, never changed.

use rand::Rng;

const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789"; // no l, o, 0, 1
pub const ID_LEN: usize = 8;

pub fn new_id() -> String {
    let mut rng = rand::thread_rng();
    (0..ID_LEN).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

pub fn is_valid(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|b| ALPHABET.contains(&b))
}
