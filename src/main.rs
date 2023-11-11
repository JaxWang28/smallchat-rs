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

struct Client {
    nick: String,
    id: u32,
}

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

async fn process_client(mut client_stream: TcpStream, tx: Sender<(u32, String)>, id: u32){
    let str = "Welcome to Simple Chat! Use /nick <nick> to set your nick.\n";
    if let Err(e) = client_stream.write_all(str.as_bytes()).await {
        eprintln!("write to client failed: {}", e);
        return;
    }

    let client = Arc::new(Mutex::new(Client{
        nick:format!("User {}", id),
        id,
    }));
    let client_clone = Arc::clone(&client);
    let (client_reader, client_writer) = client_stream.into_split();
    let rx = tx.subscribe();

    let mut receive_task = tokio::spawn(receive_from_client(client_reader, tx, client));
    let mut send_task = tokio::spawn(send_to_client(client_writer, rx, client_clone));

    // 无论是读任务还是写任务的终止，另一个任务都将没有继续存在的意义，因此都将另一个任务也终止
    if tokio::try_join!(&mut receive_task, &mut send_task).is_err() {
        eprintln!("read_task/write_task terminated");
        receive_task.abort();
        send_task.abort();
    };
}

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
                // read_line()读取时会包含换行符，因此去除行尾换行符
                // 将buf.drain(。。)会将buf清空，下一次read_line读取的内容将从头填充而不是追加
                buf.pop();
                let content = buf.drain(..).as_str().to_string();

                let prefix = "/nick ";
                let mut client = client.lock().await;
                match content.strip_prefix(prefix) {
                    Some(rest) => {
                        client.nick = rest.to_string();
                        client.nick.pop();
                    },
                    None => {
                        // 将内容发送给writer，让writer响应给客户端，
                        // 如果无法发送给writer，继续从客户端读取内容将没有意义，因此break退出
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

async fn send_to_client(mut writer: OwnedWriteHalf, mut rx: Receiver<(u32, String)>, client: Arc::<Mutex::<Client>>){
    while let Ok((id, mut str)) = rx.recv().await {
        let client = client.lock().await;
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
