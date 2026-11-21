//! "Nu speelt": leest wat Windows als actieve mediasessie kent (Spotify,
//! browser, enz.) en stuurt afspeelknoppen terug.

use base64::Engine;
use serde::Serialize;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// data-URL van de albumhoes, of leeg.
    pub art: String,
}

fn session() -> windows::core::Result<Session> {
    Manager::RequestAsync()?.join()?.GetCurrentSession()
}

fn read_thumbnail(reference: &IRandomAccessStreamReference) -> windows::core::Result<String> {
    let stream = reference.OpenReadAsync()?.join()?;
    let size = stream.Size()?.min(4 * 1024 * 1024) as u32;
    if size == 0 {
        return Ok(String::new());
    }
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    reader.LoadAsync(size)?.join()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes)?;
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

pub struct Watcher {
    last: Option<Media>,
}

impl Watcher {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// Huidige sessie, of None als er niets speelt/gepauzeerd klaarstaat.
    pub fn poll(&mut self) -> Option<Media> {
        let current = self.read().ok().filter(|m| !m.title.is_empty());
        self.last = current.clone();
        current
    }

    fn read(&self) -> windows::core::Result<Media> {
        let session = session()?;
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let title = props.Title()?.to_string();
        let artist = props.Artist()?.to_string();
        let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;

        // De albumhoes alleen opnieuw inlezen als het nummer wisselt.
        let art = match &self.last {
            Some(prev) if prev.title == title && prev.artist == artist && !prev.art.is_empty() => prev.art.clone(),
            _ => props.Thumbnail().and_then(|t| read_thumbnail(&t)).unwrap_or_default(),
        };
        Ok(Media { title, artist, playing, art })
    }
}

pub fn command(cmd: &str) -> windows::core::Result<()> {
    let s = session()?;
    match cmd {
        "next" => s.TrySkipNextAsync()?.join()?,
        "prev" => s.TrySkipPreviousAsync()?.join()?,
        _ => s.TryTogglePlayPauseAsync()?.join()?,
    };
    Ok(())
}
