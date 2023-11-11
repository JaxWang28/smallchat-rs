use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
        TcpStream,
    },
    sync::{
        broadcast::{self, Sender, Receiver},
        Mutex,
    },
};
use std::sync::Arc;

/* 用来存储一个 client 的信息 */
struct Client {
    nick: String,
    id: u32,
}


/* main 通过 #[tokio::main] 创建 tokio runtime */
#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:7878").await.unwrap();
    let (tx, _) = broadcast::channel::<(u32, String)>(16);
    let mut id: u32 = 0;
    loop {
        let (client_stream, _) = listener.accept().await.unwrap();
        tokio::spawn(process_client(client_stream, tx.clone(), id));
        id = id + 1;
    }
}

/* 处理 TcpStream, 将其分离为接收端和发送端，分别分配到 receive_task 和 send_task 中 */
async fn process_client(mut client_stream: TcpStream, tx: Sender<(u32, String)>, id: u32){
    let str = "Welcome to Simple Chat! Use /nick <nick> to set your nick.\n";
    if let Err(e) = client_stream.write_all(str.as_bytes()).await {
        eprintln!("write to client failed: {}", e);
        return;
    }

    /* 用来在 receive_task 和 send_task 中共享数据 */
    let client = Arc::new(Mutex::new(Client{
        nick:format!("User {}", id),
        id,
    }));
    let client_clone = Arc::clone(&client);
    let (client_reader, client_writer) = client_stream.into_split();
    let rx = tx.subscribe();

    /* 创建 receive_task */
    let mut receive_task = tokio::spawn(receive_from_client(client_reader, tx, client));
    /* 创建 send_task */
    let mut send_task = tokio::spawn(send_to_client(client_writer, rx, client_clone));

    /* 无论哪一个 task 终止，另一个都没有存在的意义 */
    if tokio::try_join!(&mut receive_task, &mut send_task).is_err() {
        eprintln!("read_task/write_task terminated");
        receive_task.abort();
        send_task.abort();
    };
}

/* 从 Tcp stream 中读取数据，将数据写入 Broadcast Channel */
async fn receive_from_client(reader: OwnedReadHalf, tx: Sender<(u32,String)>, client: Arc::<Mutex::<Client>>){
    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut buf = String::new();
    loop {
        match buf_reader.read_line(&mut buf).await {
            Err(_e) => {
                eprintln!("read from client error");
                break;
            }
            // 遇到了EOF
            Ok(0) => {
                println!("client closed");
                break;
            }
            Ok(_n) => {
                /* 去除行尾换行符 */
                buf.pop();
                /* 将 buf 清空，确保下次从头填充而不是追加 */
                let content = buf.drain(..).as_str().to_string();

                /* 匹配从 Tcp Stream 中传入的是否为 /nick 命令 */
                let prefix = "/nick ";
                let mut client = client.lock().await;
                match content.strip_prefix(prefix) {
                    /* /nick 命令 */
                    Some(rest) => {
                        client.nick = rest.to_string();
                        client.nick.pop();
                    },
                    None => {
                        /* 发送到 Broadcast Channel */
                        let content = format!("{} > {}", client.nick, content);
                        println!("{}", content);
                        if tx.send((client.id,content)).is_err() {
                            eprintln!("receiver closed");
                            break;
                        }
                    }
                }
            }
        }
    }
}

/* 从 Broadcast Channel 中读取数据，将数据写入 TcpStream */
async fn send_to_client(mut writer: OwnedWriteHalf, mut rx: Receiver<(u32, String)>, client: Arc::<Mutex::<Client>>){
    while let Ok((id, mut str)) = rx.recv().await {
        let client = client.lock().await;
        /* 过滤自己发送到 Broadcast 中的数据 */
        if id == client.id {
            continue;
        }
        str.push('\n');
        if let Err(e) = writer.write_all(str.as_bytes()).await {
            eprintln!("write to client failed: {}", e);
            break;
        }
    }
}
