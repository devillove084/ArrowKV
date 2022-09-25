// Copyright 2022 The template Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::{Arc, Condvar, Mutex};

use futures::channel::oneshot;

use super::error::IOKindResult;
use crate::Record;

pub struct Request {
    pub sender: oneshot::Sender<IOKindResult<u64>>,
    /// A shutdown is delivered if record is None.
    pub record: Option<Record>,
}

struct ChannelCore {
    requests: Vec<Request>,
    waitting: bool,
}

#[derive(Clone)]
pub struct Channel {
    core: Arc<(Mutex<ChannelCore>, Condvar)>,
}

impl Channel {
    pub fn new() -> Self {
        Channel {
            core: Arc::new((
                Mutex::new(ChannelCore {
                    requests: Vec::new(),
                    waitting: false,
                }),
                Condvar::new(),
            )),
        }
    }

    pub fn take(&self) -> Vec<Request> {
        let mut core = self.core.0.lock().unwrap();
        while core.requests.is_empty() {
            core.waitting = true;
            core = self.core.1.wait(core).unwrap();
        }
        std::mem::take(&mut core.requests)
    }

    pub fn append(&self, record: Record) -> oneshot::Receiver<IOKindResult<u64>> {
        let (sender, receiver) = oneshot::channel();
        let mut core = self.core.0.lock().unwrap();
        core.requests.push(Request {
            sender,
            record: Some(record),
        });
        if core.waitting {
            core.waitting = false;
            self.core.1.notify_one();
        }
        receiver
    }

    pub fn shutdown(&self) {
        let (sender, _) = oneshot::channel();
        let mut core = self.core.0.lock().unwrap();
        core.requests.push(Request {
            sender,
            record: None,
        });
        if core.waitting {
            core.waitting = false;
            self.core.1.notify_one();
        }
    }
}
