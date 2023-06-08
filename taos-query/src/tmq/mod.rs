use std::{fmt::Debug, pin::Pin, str::FromStr, time::Duration};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{
    common::{JsonMeta, RawData, RawMeta},
    RawBlock,
};

#[derive(Debug, Clone, Copy)]
pub enum Timeout {
    /// Wait forever.
    Never,
    /// Try not block, will directly return when set timeout as `None`.
    None,
    /// Wait for a duration of time.
    Duration(Duration),
}

impl Timeout {
    pub fn from_secs(secs: u64) -> Self {
        Self::Duration(Duration::from_secs(secs))
    }

    pub fn from_millis(millis: u64) -> Self {
        Self::Duration(Duration::from_millis(millis))
    }

    pub fn never() -> Self {
        Self::Never
    }

    pub fn none() -> Self {
        Self::None
    }
    pub fn as_raw_timeout(&self) -> i64 {
        match self {
            Timeout::Never => -1,
            Timeout::None => 0,
            Timeout::Duration(t) => t.as_millis() as _,
        }
    }

    pub fn as_duration(&self) -> Duration {
        match self {
            Timeout::Never => Duration::from_secs(i64::MAX as u64 / 1000),
            Timeout::None => Duration::from_secs(0),
            Timeout::Duration(t) => *t,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TimeoutError {
    #[error("empty timeout value")]
    Empty,
    #[error("invalid timeout expression `{0}`: {1}")]
    Invalid(String, String),
}

impl FromStr for Timeout {
    type Err = TimeoutError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(TimeoutError::Empty);
        }
        match s.to_lowercase().as_str() {
            "never" => Ok(Timeout::Never),
            "none" => Ok(Timeout::None),
            _ => parse_duration::parse(s)
                .map(Timeout::Duration)
                .map_err(|err| TimeoutError::Invalid(s.to_string(), err.to_string())),
        }
    }
}

pub enum MessageSet<M, D> {
    Meta(M),
    Data(D),
    MetaData(M, D),
}

impl<M, D> Debug for MessageSet<M, D>
where
    M: Debug,
    D: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Meta(m) => f.debug_tuple("Meta").field(m).finish(),
            Self::Data(d) => f.debug_tuple("Data").field(d).finish(),
            Self::MetaData(m, d) => f.debug_tuple("MetaData").field(m).field(d).finish(),
        }
    }
}

impl<M, D> MessageSet<M, D> {
    pub fn into_meta(self) -> Option<M> {
        match self {
            MessageSet::Meta(m) => Some(m),
            MessageSet::Data(_) => None,
            MessageSet::MetaData(m, _) => Some(m),
        }
    }
    pub fn into_data(self) -> Option<D> {
        match self {
            MessageSet::Meta(_) => None,
            MessageSet::Data(d) => Some(d),
            MessageSet::MetaData(_, d) => Some(d),
        }
    }

    pub fn has_meta(&self) -> bool {
        matches!(self, &MessageSet::Meta(_) | &MessageSet::MetaData(_, _))
    }
    pub fn has_data(&self) -> bool {
        matches!(self, &MessageSet::Data(_) | &MessageSet::MetaData(_, _))
    }

    pub fn meta(&self) -> Option<&M> {
        match self {
            MessageSet::Meta(m) => Some(m),
            MessageSet::Data(_) => None,
            MessageSet::MetaData(m, _) => Some(m),
        }
    }
    pub fn data(&mut self) -> Option<&mut D> {
        match self {
            MessageSet::Meta(_) => None,
            MessageSet::Data(d) => Some(d),
            MessageSet::MetaData(_, d) => Some(d),
        }
    }
}

#[async_trait::async_trait]
pub trait IsAsyncMeta {
    type Error;

    async fn as_raw_meta(&self) -> Result<RawMeta, Self::Error>;

    async fn as_json_meta(&self) -> Result<JsonMeta, Self::Error>;
}

impl<T> IsMeta for T
where
    T: IsAsyncMeta + SyncOnAsync,
{
    type Error = T::Error;

    fn as_raw_meta(&self) -> Result<RawMeta, Self::Error> {
        crate::block_in_place_or_global(T::as_raw_meta(self))
    }

    fn as_json_meta(&self) -> Result<JsonMeta, Self::Error> {
        crate::block_in_place_or_global(T::as_json_meta(self))
    }
}

#[async_trait::async_trait]
impl<T> IsAsyncMeta for T
where
    T: IsMeta + AsyncOnSync + Send + Sync,
{
    type Error = T::Error;

    async fn as_raw_meta(&self) -> Result<RawMeta, Self::Error> {
        <T as IsMeta>::as_raw_meta(self)
    }

    async fn as_json_meta(&self) -> Result<JsonMeta, Self::Error> {
        <T as IsMeta>::as_json_meta(self)
    }
}

pub trait IsMeta {
    type Error;

    fn as_raw_meta(&self) -> Result<RawMeta, Self::Error>;

    fn as_json_meta(&self) -> Result<JsonMeta, Self::Error>;
}

#[async_trait::async_trait]
pub trait IsAsyncData {
    type Error;

    async fn as_raw_data(&self) -> Result<RawData, Self::Error>;
    async fn fetch_raw_block(&self) -> Result<Option<RawBlock>, Self::Error>;
}

pub trait IsData {
    type Error;

    fn as_raw_data(&self) -> Result<RawData, Self::Error>;
    fn fetch_raw_block(&self) -> Result<Option<RawBlock>, Self::Error>;
}

#[async_trait::async_trait]
pub trait AsyncMessage {
    type Error;

    /// Check if the message contains meta.
    fn has_meta(&self) -> bool;
    /// Check if the message contains data.
    fn has_data(&self) -> bool;

    /// Return raw data as bytes.
    async fn as_raw_data(&self) -> Result<RawData, Self::Error>;

    /// Extract meta message.
    async fn get_meta(&self) -> Result<Option<RawMeta>, Self::Error>;
    async fn fetch_raw_block(&self) -> Result<Option<RawBlock>, Self::Error>;
}

pub type VGroupId = i32;

/// Extract offset information.
pub trait IsOffset {
    /// Database name for current message
    fn database(&self) -> &str;

    /// Topic name for current message.
    fn topic(&self) -> &str;

    /// VGroup id for current message.
    fn vgroup_id(&self) -> VGroupId;
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Deserialize, Serialize)]
pub struct Assignment {
    vgroup_id: VGroupId,
    offset: i64,
    begin: i64,
    end: i64,
}

impl Assignment {
    pub fn new(vgroup_id: VGroupId, offset: i64, begin: i64, end: i64) -> Self {
        Self {
            vgroup_id,
            offset,
            begin,
            end,
        }
    }

    pub fn vgroup_id(&self) -> VGroupId {
        self.vgroup_id
    }

    pub fn current_offset(&self) -> i64 {
        self.offset
    }

    pub fn begin(&self) -> i64 {
        self.begin
    }

    pub fn end(&self) -> i64 {
        self.end
    }
}

pub trait AsConsumer: Sized {
    type Error;
    type Offset: IsOffset;
    type Meta: IsMeta;
    type Data: IntoIterator<Item = Result<RawBlock, Self::Error>>;

    /// Default timeout getter for message stream.
    fn default_timeout(&self) -> Timeout {
        Timeout::Never
    }

    fn subscribe<T: Into<String>, I: IntoIterator<Item = T> + Send>(
        &mut self,
        topics: I,
    ) -> Result<(), Self::Error>;

    /// None means wait until next message come.
    fn recv_timeout(
        &self,
        timeout: Timeout,
    ) -> Result<Option<(Self::Offset, MessageSet<Self::Meta, Self::Data>)>, Self::Error>;

    fn recv(
        &self,
    ) -> Result<Option<(Self::Offset, MessageSet<Self::Meta, Self::Data>)>, Self::Error> {
        self.recv_timeout(self.default_timeout())
    }

    fn iter_data_only(
        &self,
        timeout: Timeout,
    ) -> Box<dyn '_ + Iterator<Item = Result<(Self::Offset, Self::Data), Self::Error>>> {
        Box::new(
            self.iter_with_timeout(timeout)
                .filter_map_ok(|m| m.1.into_data().map(|data| (m.0, data))),
        )
    }

    fn iter_with_timeout(&self, timeout: Timeout) -> MessageSetsIter<'_, Self> {
        MessageSetsIter {
            consumer: self,
            timeout: timeout,
        }
    }

    fn iter(&self) -> MessageSetsIter<'_, Self> {
        self.iter_with_timeout(self.default_timeout())
    }

    fn commit(&self, offset: Self::Offset) -> Result<(), Self::Error>;

    fn unsubscribe(self) {
        drop(self)
    }

    fn assignments(&self) -> Option<Vec<(String, Vec<Assignment>)>>;

    fn offset_seek(&mut self, topic: &str, vg_id: VGroupId, offset: i64)
        -> Result<(), Self::Error>;
}

pub struct MessageSetsIter<'a, C> {
    consumer: &'a C,
    timeout: Timeout,
}

impl<'a, C> Iterator for MessageSetsIter<'a, C>
where
    C: AsConsumer,
{
    type Item = Result<(C::Offset, MessageSet<C::Meta, C::Data>), C::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.consumer.recv_timeout(self.timeout).transpose()
    }
}

#[async_trait::async_trait]
pub trait AsAsyncConsumer: Sized + Send + Sync {
    type Error;
    type Offset: IsOffset;
    type Meta: IsAsyncMeta;
    type Data: IsAsyncData;

    fn default_timeout(&self) -> Timeout;

    async fn subscribe<T: Into<String>, I: IntoIterator<Item = T> + Send>(
        &mut self,
        topics: I,
    ) -> Result<(), Self::Error>;

    /// None means wait until next message come.
    async fn recv_timeout(
        &self,
        timeout: Timeout,
    ) -> Result<Option<(Self::Offset, MessageSet<Self::Meta, Self::Data>)>, Self::Error>;

    fn stream_with_timeout(
        &self,
        timeout: Timeout,
    ) -> Pin<
        Box<
            dyn '_
                + Send
                + futures::Stream<
                    Item = Result<(Self::Offset, MessageSet<Self::Meta, Self::Data>), Self::Error>,
                >,
        >,
    > {
        Box::pin(futures::stream::unfold((), move |_| async move {
            let weather = self.recv_timeout(timeout).await.transpose();
            weather.map(|res| (res, ()))
        }))
    }

    fn stream(
        &self,
    ) -> Pin<
        Box<
            dyn '_
                + Send
                + futures::Stream<
                    Item = Result<(Self::Offset, MessageSet<Self::Meta, Self::Data>), Self::Error>,
                >,
        >,
    > {
        self.stream_with_timeout(self.default_timeout())
    }

    async fn commit(&self, offset: Self::Offset) -> Result<(), Self::Error>;

    async fn unsubscribe(self) {
        drop(self)
    }

    async fn assignments(&self) -> Option<Vec<(String, Vec<Assignment>)>>;

    async fn topic_assignment(&self, topic: &str) -> Vec<Assignment>;

    async fn offset_seek(
        &mut self,
        topic: &str,
        vgroup_id: VGroupId,
        offset: i64,
    ) -> Result<(), Self::Error>;
}

/// Marker trait to impl sync on async impl.
pub trait SyncOnAsync {}

pub trait AsyncOnSync {}

impl<C> AsConsumer for C
where
    C: AsAsyncConsumer + SyncOnAsync,
    C::Meta: IsMeta,
    C::Data: IntoIterator<Item = Result<RawBlock, C::Error>>,
{
    type Error = C::Error;

    type Offset = C::Offset;

    type Meta = C::Meta;

    type Data = C::Data;

    fn subscribe<T: Into<String>, I: IntoIterator<Item = T> + Send>(
        &mut self,
        topics: I,
    ) -> Result<(), Self::Error> {
        crate::block_in_place_or_global(<C as AsAsyncConsumer>::subscribe(self, topics))
    }

    fn recv_timeout(
        &self,
        timeout: Timeout,
    ) -> Result<Option<(Self::Offset, MessageSet<Self::Meta, Self::Data>)>, Self::Error> {
        crate::block_in_place_or_global(<C as AsAsyncConsumer>::recv_timeout(self, timeout))
    }

    fn commit(&self, offset: Self::Offset) -> Result<(), Self::Error> {
        crate::block_in_place_or_global(<C as AsAsyncConsumer>::commit(self, offset))
    }

    fn assignments(&self) -> Option<Vec<(String, Vec<Assignment>)>> {
        crate::block_in_place_or_global(<C as AsAsyncConsumer>::assignments(self))
    }

    fn offset_seek(
        &mut self,
        topic: &str,
        vg_id: VGroupId,
        offset: i64,
    ) -> Result<(), Self::Error> {
        crate::block_in_place_or_global(<C as AsAsyncConsumer>::offset_seek(
            self, topic, vg_id, offset,
        ))
    }
}

// #[async_trait::async_trait]
// impl<C> AsAsyncConsumer for C
// where
//     C: AsConsumer + AsyncOnSync + Send + Sync + 'static,
//     C::Error: 'static + Sync + Send,
//     C::Meta: IsAsyncMeta + Send,
//     C::Offset: 'static + Sync + Send,
//     C::Data: 'static + Send + Sync,
// {
//     type Error = C::Error;

//     type Offset = C::Offset;

//     type Meta = C::Meta;

//     type Data = C::Data;

//     async fn subscribe<T: Into<String>, I: IntoIterator<Item = T> + Send>(
//         &mut self,
//         topics: I,
//     ) -> Result<(), Self::Error> {
//         <C as AsConsumer>::subscribe(self, topics)
//     }

//     async fn recv_timeout(
//         &self,
//         timeout: Timeout,
//     ) -> Result<Option<(Self::Offset, MessageSet<Self::Meta, Self::Data>)>, Self::Error> {
//         <C as AsConsumer>::recv_timeout(self, timeout)
//     }

//     async fn commit(&self, offset: Self::Offset) -> Result<(), Self::Error> {
//         <C as AsConsumer>::commit(self, offset)
//     }
// }
