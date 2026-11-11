//! Plays feedback sounds one at a time, in the order they were asked for.
//!
//! Handy plays each sound on a thread of its own, so two sounds can overlap.
//! DanyWhere's clipboard sound follows the stop sound closely: after a short
//! dictation the transcript can be ready before the stop sound has rung out.
//! A single player thread keeps the sounds apart.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread;

use log::error;

/// Tells a waiting caller that its sound has played.
type Done = mpsc::Sender<()>;

/// A queue of sounds (`J` describes one) with a thread that plays them.
pub struct SoundQueue<J> {
    jobs: mpsc::Sender<(J, Option<Done>)>,
}

impl<J: Send + 'static> SoundQueue<J> {
    /// Starts the player thread, which runs `play` for each queued job.
    pub fn new(play: impl Fn(J) + Send + 'static) -> std::io::Result<Self> {
        let (jobs, queued) = mpsc::channel::<(J, Option<Done>)>();
        thread::Builder::new()
            .name("feedback-sounds".into())
            .spawn(move || {
                for (job, done) in queued {
                    // A sound that fails must not end the thread, or every
                    // later sound would be lost.
                    if catch_unwind(AssertUnwindSafe(|| play(job))).is_err() {
                        error!("A feedback sound panicked; playing the next one");
                    }
                    if let Some(done) = done {
                        // The caller may have stopped waiting; nothing to do then.
                        let _ = done.send(());
                    }
                }
            })?;
        Ok(Self { jobs })
    }

    /// Queues `job` and returns at once. Gives the job back if the player
    /// thread is gone.
    pub fn enqueue(&self, job: J) -> Result<(), J> {
        self.jobs
            .send((job, None))
            .map_err(|mpsc::SendError((job, _))| job)
    }

    /// Queues `job` and returns once it has played. Gives the job back if the
    /// player thread is gone.
    pub fn play_and_wait(&self, job: J) -> Result<(), J> {
        let (done, played) = mpsc::channel();
        self.jobs
            .send((job, Some(done)))
            .map_err(|mpsc::SendError((job, _))| job)?;
        // An error here means the thread ended while playing; stop waiting.
        let _ = played.recv();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SoundQueue;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    /// A queue whose "sounds" take `ms` milliseconds each and write their
    /// start and end into the returned log.
    fn recording_queue(ms: &'static [(u32, u64)]) -> (SoundQueue<u32>, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let played = Arc::clone(&log);
        let queue = SoundQueue::new(move |n: u32| {
            let length = ms.iter().find(|(id, _)| *id == n).map_or(10, |(_, ms)| *ms);
            played.lock().unwrap().push(format!("start {n}"));
            thread::sleep(Duration::from_millis(length));
            played.lock().unwrap().push(format!("end {n}"));
        })
        .unwrap();
        (queue, log)
    }

    #[test]
    fn sounds_play_one_after_another_in_request_order() {
        let (queue, log) = recording_queue(&[(1, 40), (2, 20), (3, 10)]);
        queue.enqueue(1).unwrap();
        queue.enqueue(2).unwrap();
        queue.play_and_wait(3).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["start 1", "end 1", "start 2", "end 2", "start 3", "end 3"]
        );
    }

    #[test]
    fn waiting_returns_once_the_sound_has_played_after_earlier_ones() {
        // The stop sound is still playing when the clipboard sound is asked for.
        let (queue, log) = recording_queue(&[(1, 60), (2, 10)]);
        queue.enqueue(1).unwrap();
        queue.play_and_wait(2).unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            ["start 1", "end 1", "start 2", "end 2"]
        );
    }

    #[test]
    fn a_failing_sound_does_not_silence_later_ones() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let played = Arc::clone(&log);
        let queue = SoundQueue::new(move |n: u32| {
            if n == 1 {
                panic!("test: sound {n} fails");
            }
            played.lock().unwrap().push(n);
        })
        .unwrap();
        queue.enqueue(1).unwrap();
        queue.play_and_wait(2).unwrap();
        assert_eq!(*log.lock().unwrap(), [2]);
    }
}
