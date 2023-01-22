use chrono::{DateTime, Duration, Local};
use machine::{machine, transitions};

machine!(
    #[derive(Clone, Copy, PartialEq, Debug)]
    pub(crate) enum RecordingState {
        A {
            pub since: DateTime<Local>,
        },
        B1 {
            pub since: DateTime<Local>,
        },
        B2 {
            pub since: DateTime<Local>,
        },
        Rec {
            pub since: DateTime<Local>,
            pub end: DateTime<Local>,
        },
        Timeout {},
        Success {},
    }
);
impl IntoB2 for A {}
impl IntoB2 for B1 {}

impl IntoRec for A {}
impl IntoRec for B1 {}
impl IntoRec for B2 {}

trait IntoB2 {
    fn on_found_in_following(self, _: FoundInFollowing) -> B2
    where
        Self: Sized,
    {
        B2 {
            since: Local::now(),
        }
    }
}

trait IntoRec {
    fn on_found_in_present(self, present: FoundInPresent) -> Rec
    where
        Self: Sized,
    {
        Rec {
            since: Local::now(),
            end: present
                .duration
                .map_or(Local::now() + Duration::minutes(5), |d| {
                    present.start_at + d
                }),
        }
    }
}

impl A {
    fn on_wait_for_premiere(self, WaitForPremiere { start_at }: WaitForPremiere) -> RecordingState {
        if start_at < Local::now() {
            RecordingState::B1(B1 {
                since: Local::now(),
            })
        } else if self.since + Duration::hours(1) < Local::now() {
            RecordingState::Timeout(self::Timeout {})
        } else {
            RecordingState::A(A { since: self.since })
        }
    }
}
impl B1 {
    fn on_wait_for_premiere(self, WaitForPremiere { start_at }: WaitForPremiere) -> RecordingState {
        if self.since + Duration::hours(3) < Local::now() {
            RecordingState::Timeout(self::Timeout {})
        } else {
            RecordingState::B1(B1 { since: self.since })
        }
    }
}
impl B2 {
    fn on_found_in_following(self, _: FoundInFollowing) -> B2 {
        self
    }
}
impl Rec {
    fn on_found_in_present(self, _: FoundInPresent) -> Rec {
        self
    }
    fn on_wait_for_premiere(self, WaitForPremiere { start_at }: WaitForPremiere) -> Success {
        Success {}
    }
}

transitions!(RecordingState,
    [
        (A, FoundInPresent) => Rec,
        (B1, FoundInPresent) => Rec,
        (B2, FoundInPresent) => Rec,
        (A, WaitForPremiere) => [A, B1],
        (B1, WaitForPremiere) => [B1, Timeout],
        (Rec, WaitForPremiere) => Success,
        (A, FoundInFollowing) => B2,
        (B1, FoundInFollowing) => B2,
        (B2, FoundInFollowing) => B2,
        (A, FoundInPresent) => Rec,
        (B1, FoundInPresent) => Rec,
        (B2, FoundInPresent) => Rec,
        (Rec, FoundInPresent) => Rec
    ]
);

#[derive(Clone, Debug, PartialEq)]
pub struct FoundInFollowing {
    pub start_at: DateTime<Local>,
    pub duration: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FoundInPresent {
    pub start_at: DateTime<Local>,
    pub duration: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaitForPremiere {
    pub start_at: DateTime<Local>,
}
