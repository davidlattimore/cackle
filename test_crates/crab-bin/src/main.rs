pmacro1::create_write_to_file!();

fn main() {
    let values = [1, 2, 42];
    // This unsafe is here to make sure that we handle unsafe code in packages with hyphens in their
    // name correctly. This is easy to mess up since the crate name passed to rustc will have an
    // underscore instead of a hyphen.
    let value = crab1::crab1(*unsafe { values.get_unchecked(2) });
    println!("{value}");
    write_to_file("a.txt", "Hello");
    crab2::stuff::do_stuff();
    crab4::access_file();
    non_mangled_function();
}

#[no_mangle]
fn non_mangled_function() {
    // Make sure we don't miss function references from non-mangled functions.
    println!("{:?}", std::env::var("HOME"));
}
