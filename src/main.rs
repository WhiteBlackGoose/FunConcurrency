use std::thread;

fn main() {
    let avec = rst_test::AVec::new(10);
    thread::scope(|s| {
        s.spawn(|| {
            avec.push(2);
            avec.push(3);
            avec.push(4);
            let el = avec.get(2).unwrap();
            avec.push(5);
            println!("Aaa {}", *el);
            avec.push(1);
        });
    });
    avec.push(1);
    avec.push(2);
    avec.push(3);
    avec.push(4);
    for i in 0..avec.len() {
        println!("#{} element: {}", i, *avec.get(i).unwrap());
    }
}
