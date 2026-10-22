use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zilla_rs::demosrv::DemoFtp;

async fn read_line(s: &mut tokio::net::TcpStream) -> String {
    let mut b = vec![0u8; 256];
    match tokio::time::timeout(std::time::Duration::from_secs(3), s.read(&mut b)).await {
        Ok(Ok(n)) if n > 0 => String::from_utf8_lossy(&b[..n]).trim_end().to_string(),
        _ => "<aucune réponse>".to_string(),
    }
}

#[tokio::test]
async fn trois_connections_demo() {
    let root = std::env::temp_dir().join(format!("zilla-demo3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("f.txt"), b"bonjour\n").unwrap();
    let demo = DemoFtp::start(&root).await.unwrap();
    let port = demo.port();

    let mut oks = 0;
    for i in 0..5 {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.set_nodelay(true).unwrap();
        let banner = read_line(&mut s).await;
        println!("#{i} bannière : {banner}");
        s.write_all(b"USER anonymous\r\n").await.unwrap();
        let r = read_line(&mut s).await;
        println!("   USER -> {r}");
        s.write_all(b"PASS a@b\r\n").await.unwrap();
        let r = read_line(&mut s).await;
        println!("   PASS -> {r}");
        if banner.starts_with("220") {
            oks += 1;
        }
        let _ = s.write_all(b"QUIT\r\n").await;
        drop(s);
    }
    assert_eq!(oks, 5);
    let _ = std::fs::remove_dir_all(&root);
}
