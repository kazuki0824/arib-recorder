mod tsduck;

use crate::recording_pool::recording_task::eit_parser::tsduck::TsDuckInner;
use crate::recording_pool::recording_task::{FoundInFollowing, FoundInPresent};
use crate::RecordingTaskDescription;
use chrono::{DateTime, Local};
use log::error;
use std::io::Error;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;
use tokio::sync::watch::{Receiver, Sender};

enum EitParserInner {
    SelfImpl,
    TsDuck(TsDuckInner),
}

pub(crate) struct EitParser {
    parser: EitParserInner,
    pub(super) rx: Receiver<EitDetected>,
}

#[derive(Debug)]
pub enum EitDetected {
    P(FoundInPresent),
    F(FoundInFollowing),
    NotFound { since: DateTime<Local> },
}

impl EitParser {
    pub fn new() -> Result<Self, Error> {
        let (tx, rx) = tokio::sync::watch::channel(EitDetected::NotFound {
            since: Local::now(),
        });

        let parser = match TsDuckInner::new(tx) {
            Ok(inner) => EitParserInner::TsDuck(inner),
            Err(e) => {
                error!("{}", e);
                return Err(e);
            }
        };

        Ok(EitParser { parser, rx })
    }
}

impl AsyncWrite for EitParser {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        match self.get_mut().parser {
            EitParserInner::SelfImpl => todo!(),
            EitParserInner::TsDuck(ref mut inner) => Pin::new(inner).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut().parser {
            EitParserInner::SelfImpl => todo!(),
            EitParserInner::TsDuck(ref mut inner) => Pin::new(inner).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut().parser {
            EitParserInner::SelfImpl => todo!(),
            EitParserInner::TsDuck(ref mut inner) => Pin::new(inner).poll_shutdown(cx),
        }
    }
}
