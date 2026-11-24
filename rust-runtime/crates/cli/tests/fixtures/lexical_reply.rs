use std::{
    env,
    io::{self, BufRead},
};

fn main() {
    let frame = env::var("LW_TEST_MODE").unwrap();
    println!(r#"{{"success":true}}"#);
    let mut input = io::stdin().lock().lines();
    if input.next().is_some() {
        println!("{frame}");
        // An ignored matching bad frame would be masked by this valid reply.
        println!(r#"{{"request_id":1,"success":true,"fallback":true}}"#);
    }
    let _ = input.next(); // Wait for the CLI's normal cleanup request.
}
