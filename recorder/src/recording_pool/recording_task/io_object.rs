use std::io::Error;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use log::{info, warn};
use tokio::fs::File;
use tokio::io::{AsyncWrite, BufWriter, Sink};
use tokio::process::{Child, Command};

pub(super) enum IoObject {
    Raw(BufWriter<File>),
    WithFilter(Child),
    Null(Sink)
}

impl IoObject {
    pub(super) async fn new(output: Option<&Path>) -> Result<Self, Error> {
        info!("File open: {:?}", output);

        if let Some(output) = output {
            let writer = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .write(true)
                .open(output)?;

            let child = Command::new(
                "/home/maleicacid/CLionProjects/recorder-backend-rs/target/debug/tsreadex",
            )
                .stdout(writer)
                .args(vec![
                    // 取り除く TS パケットの10進数の PID
                    // EIT の PID を指定
                    "-x",
                    "18/38/39",
                    // 特定サービスのみを選択して出力するフィルタを有効にする
                    // 有効にすると、特定のストリームのみ PID を固定して出力される
                    "-n",
                    "-1",
                    // 主音声ストリームが常に存在する状態にする
                    // ストリームが存在しない場合、無音の AAC ストリームが出力される
                    // 音声がモノラルであればステレオにする
                    // デュアルモノを2つのモノラル音声に分離し、右チャンネルを副音声として扱う
                    "-a",
                    "13",
                    // 副音声ストリームが常に存在する状態にする
                    // ストリームが存在しない場合、無音の AAC ストリームが出力される
                    // 音声がモノラルであればステレオにする
                    "-b",
                    "5",
                    // 字幕ストリームが常に存在する状態にする
                    // ストリームが存在しない場合、PMT の項目が補われて出力される
                    "-c",
                    "1",
                    // 文字スーパーストリームが常に存在する状態にする
                    // ストリームが存在しない場合、PMT の項目が補われて出力される
                    "-u",
                    "1",
                    // 字幕と文字スーパーを aribb24.js が解釈できる ID3 timed-metadata に変換する
                    // +4: FFmpeg のバグを打ち消すため、変換後のストリームに規格外の5バイトのデータを追加する
                    // +8: FFmpeg のエラーを防ぐため、変換後のストリームの PTS が単調増加となるように調整する
                    "-d",
                    "13",
                    output.to_str().unwrap(),
                ])
                .kill_on_drop(true)
                .spawn();

            Ok(match child {
                Ok(p) => Self::WithFilter(p),
                Err(e) => {
                    warn!("Spawn error. {}", e);
                    Self::Raw(BufWriter::new(
                        tokio::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .write(true)
                            .open(output)
                            .await?
                    ))
                }
            })
        } else {
            Ok(Self::Null(tokio::io::sink()))
        }
    }
}

impl AsyncWrite for IoObject {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        match self.get_mut() {
            Self::WithFilter(Child {
                stdin: Some(ref mut stdin),
                ..
            }) => Pin::new(stdin).poll_write(cx, buf),
            Self::Raw(ref mut raw_out) => Pin::new(raw_out).poll_write(cx, buf),
            Self::Null(sink) => Pin::new(sink).poll_write(cx, buf),
            _ => panic!("This process has no stdin."),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut() {
            Self::WithFilter(Child {
                stdin: Some(ref mut stdin),
                ..
            }) => Pin::new(stdin).poll_flush(cx),
            Self::Raw(ref mut raw_out) => Pin::new(raw_out).poll_flush(cx),
            Self::Null(sink) => Pin::new(sink).poll_flush(cx),
            _ => panic!("This process has no stdin."),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut() {
            Self::WithFilter(Child {
                stdin: Some(ref mut stdin),
                ..
            }) => Pin::new(stdin).poll_shutdown(cx),
            Self::Raw(ref mut raw_out) => Pin::new(raw_out).poll_shutdown(cx),
            Self::Null(sink) => Pin::new(sink).poll_shutdown(cx),
            _ => panic!("This process has no stdin."),
        }
    }
}
