// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use std::sync::atomic::Ordering;

use atomic_enum::atomic_enum;

#[atomic_enum]
#[derive(PartialEq)]
pub enum Status {
    Created,
    Starting,
    Running,
    Stopping,
    Failing,
    Restarting,
    Stopped,
    Failed,
}

#[derive(Debug)]
pub struct StatusMonitor {
    status: AtomicStatus,
}

impl StatusMonitor {
    #[inline]
    pub fn status(&self) -> Status {
        self.status.load(Ordering::Relaxed)
    }

    pub fn initiate_starting(&self) -> bool {
        let mut status = self.status();

        loop {
            let previous = status;
            match status {
                Status::Created | Status::Stopped | Status::Failed => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Starting, Ordering::Relaxed);
                    if status == previous {
                        // Starting has been initiated successfully
                        return true;
                    }
                }
                Status::Stopping | Status::Failing => {
                    // Try to change state to `Restarting`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Restarting, Ordering::Relaxed);
                    if status == previous {
                        break;
                    }
                }
                // Client is currently started
                Status::Starting | Status::Running | Status::Restarting => break,
            };
            // Try it again because another task change the state
        }

        // Starting cannot be done
        false
    }

    pub fn initiate_stopping(&self) -> bool {
        let mut status = self.status();

        loop {
            let previous = status;
            match status {
                Status::Created
                | Status::Stopping
                | Status::Failing
                | Status::Stopped
                | Status::Failed => break,
                // Client is currently started
                Status::Starting | Status::Running | Status::Restarting => {
                    status =
                        self.status
                            .compare_and_swap(status, Status::Stopping, Ordering::Relaxed);
                    if status == previous {
                        // Stopping has been initiated successfully
                        return true;
                    }
                }
            };
            // Try it again because another task change the state
        }

        // Stopping cannot be done
        false
    }

    pub fn is_shutting_down(&self) -> bool {
        match self.status() {
            Status::Stopping | Status::Failing => true,
            Status::Created
            | Status::Starting
            | Status::Running
            | Status::Restarting
            | Status::Stopped
            | Status::Failed => false,
        }
    }

    pub fn can_stop(&self) -> bool {
        let mut status = self.status();

        loop {
            let previous = status;
            match status {
                Status::Created
                | Status::Starting
                | Status::Running
                | Status::Stopped
                | Status::Failed => panic!("BUG: 'can_stop': unexpected state '{:?}'", status),
                Status::Stopping => {
                    // Try to change state to `Stopped`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Stopped, Ordering::Relaxed);
                    if status == previous {
                        return true;
                    }
                }
                Status::Failing => {
                    // Try to change state to `Failed`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Failed, Ordering::Relaxed);
                    if status == previous {
                        return true;
                    }
                }
                // Restarting has been initiated
                Status::Restarting => {
                    // Try to change state to `Starting`
                    status =
                        self.status
                            .compare_and_swap(status, Status::Starting, Ordering::Relaxed);
                    if status == previous {
                        break;
                    }
                }
            };
            // Try it again because another task change the state
        }

        // Stop cannot be done
        false
    }
}

impl Default for StatusMonitor {
    fn default() -> Self {
        Self {
            status: AtomicStatus::new(Status::Created),
        }
    }
}
