use ruff_db::system::SystemPathBuf;

fn main() {
    let arg = std::env::args().nth(1).expect("usage: pylspt <path>");
    let path = SystemPathBuf::from(arg);

    println!("path       ={}", path.as_str());
    println!("file_name  ={:?}", path.file_name());
    println!("extension  ={:?}", path.extension());
    println!("parent     ={:?}", path.parent().map(|p| p.as_str()));
}
