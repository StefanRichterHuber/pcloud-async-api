use std::fmt::Display;
use std::time::Duration;

use crate::pcloud_client::PCloudClient;
use crate::pcloud_model::DiffEntry;
use crate::pcloud_model::{self, Diff};
use chrono::{DateTime, TimeZone};
use log::{debug, warn};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;

/// Consumes a Receiver of DiffEntries, applies the given predicate on each entry and passes all accepted entries to the returned Receiver
pub fn filter_stream<P>(mut source: Receiver<DiffEntry>, filter: P) -> Receiver<DiffEntry>
where
    P: Fn(&DiffEntry) -> bool + Send + 'static,
{
    let channel_size = 128;
    let (tx, rx) = mpsc::channel::<DiffEntry>(channel_size);

    tokio::spawn(async move {
        while let Some(entry) = source.recv().await {
            if filter(&entry) {
                match tx.send(entry).await {
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    });

    rx
}

pub struct DiffRequestBuilder {
    /// Client to actually perform the request
    client: PCloudClient,
    /// receive only changes since that diffid.
    diff_id: Option<u64>,
    /// datetime receive only events generated after that time
    after: Option<String>,
    /// return last number of events with highest diffids (that is the last events)
    last: Option<u64>,
    /// if set, the connection will block until an event arrives. Works only with diffid
    block: bool,
    /// block is set, provide a connection time out duration
    timeout: Option<Duration>,
    /// if provided, no more than limit entries will be returned
    limit: Option<u64>,
}

#[allow(dead_code)]
impl DiffRequestBuilder {
    pub(crate) fn create(client: &PCloudClient) -> DiffRequestBuilder {
        DiffRequestBuilder {
            diff_id: None,
            after: None,
            last: None,
            block: false,
            limit: None,
            timeout: None,
            client: client.clone(),
        }
    }

    /// receive only changes since that diffid.
    pub fn after_diff_id(mut self, value: u64) -> DiffRequestBuilder {
        self.diff_id = Some(value);
        self
    }
    /// datetime receive only events generated after that time
    pub fn after<Tz>(mut self, value: &DateTime<Tz>) -> DiffRequestBuilder
    where
        Tz: TimeZone,
        Tz::Offset: Display,
    {
        self.after = Some(pcloud_model::format_date_time_for_pcloud(value));
        self
    }

    ///  return last number of events with highest diffids (that is the last events)
    pub fn only_last(mut self, value: u64) -> DiffRequestBuilder {
        self.last = Some(value);
        self
    }

    /// if set, the connection will block until an event arrives. Works only with diffid
    pub fn block(mut self, value: bool) -> DiffRequestBuilder {
        self.block = value;
        self
    }

    /// block is set, one should provide a connection time out duration. This is especially necessary during event streaming, or the endless blocks could happen.
    pub fn block_timeout(mut self, value: Duration) -> DiffRequestBuilder {
        self.timeout = Some(value);
        self
    }

    /// if provided, no more than limit entries will be returned. If not provided ~100 entries are returned.
    pub fn limit(mut self, value: u64) -> DiffRequestBuilder {
        self.limit = Some(value);
        self
    }

    /// Streams a single batch of DiffEntries to the given Sender and returns the last diff id received
    async fn stream_once(
        self,
        tx: &Sender<DiffEntry>,
    ) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        let diff_id = self.diff_id.clone();
        let diffs = self.get().await?;

        if diffs.entries.len() > 0 {
            if !tx.is_closed() {
                debug!("Received {} events since last call", diffs.entries.len());
                for entry in diffs.entries.into_iter() {
                    if let Some(old_diff_id) = diff_id {
                        if entry.diffid > old_diff_id {
                            debug!("Received event {} -> {:?}", entry.diffid, entry.event);
                            tx.send(entry).await?;
                        }
                    } else {
                        debug!("Received event {} -> {:?}", entry.diffid, entry.event);
                        tx.send(entry).await?;
                    }
                }
            }
            Ok(Some(diffs.diffid))
        } else {
            Ok(diff_id)
        }
    }

    /// Streams the events using the given configuration. Calls the /diff endpoint repeatedly until the channel is closed.
    pub fn stream(self) -> Receiver<DiffEntry> {
        // Configure size of the channel. If a batch size is set, channel size is batch size to avoid unnecessary blocking
        let channel_size = if let Some(limit) = self.limit {
            limit as usize
        } else {
            // Without limit pCloud returns ~100 entries
            128
        };

        let (tx, rx) = mpsc::channel::<DiffEntry>(channel_size);

        tokio::spawn(async move {
            let mut next_diff_id = self.diff_id;
            while !tx.is_closed() {
                let next = DiffRequestBuilder {
                    /// There seem to be collisions when setting both after and diff_id
                    after: if next_diff_id.is_some() {
                        None
                    } else {
                        self.after.clone()
                    },
                    diff_id: next_diff_id.clone(),
                    client: self.client.clone(),
                    block: true,
                    last: self.last.clone(),
                    limit: self.limit.clone(),
                    timeout: self.timeout.clone(),
                };

                match next.stream_once(&tx).await {
                    Ok(diff_id) => {
                        next_diff_id = diff_id;
                    }
                    Err(e) => {
                        if let Some(err) = e.downcast_ref::<reqwest::Error>() {
                            // Ignore timeout errors and try next time
                            if !err.is_timeout() {
                                warn!("Connection errors during receiving events: {}", err);
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
        });

        rx
    }

    /// Fetches the events. No matter you configure the limit, not all events could be fetched at once. Therefore one has to call repeatedly with the diffid of the last result set in the next call.
    pub async fn get(self) -> Result<Diff, Box<dyn std::error::Error>> {
        let url = format!("{}/diff", self.client.api_host);
        let mut r = self.client.client.get(url);

        if let Some(v) = self.diff_id {
            r = r.query(&[("diffid", v)]);
        }

        // There seem to be collisions when setting both after and diff_id
        if let Some(v) = self.after {
            r = r.query(&[("after", v)]);
        }

        if let Some(v) = self.last {
            r = r.query(&[("last", v)]);
        }

        if let Some(v) = self.limit {
            r = r.query(&[("limit", v)]);
        }

        // if set, the connection will block until an event arrives. Works only with diffid
        if self.block && self.diff_id.is_some() {
            r = r.query(&[("block", "1")]);
        }

        if let Some(timeout) = self.timeout {
            r = r.timeout(timeout);
        }

        r = self.client.add_token(r);

        let diff = r.send().await?.json::<pcloud_model::Diff>().await?;

        Ok(diff)
    }
}

impl PCloudClient {
    /// List events on the users pCloud account.
    /// see https://docs.pcloud.com/methods/general/diff.html for details
    pub fn diff(&self) -> DiffRequestBuilder {
        DiffRequestBuilder::create(self)
    }
}
