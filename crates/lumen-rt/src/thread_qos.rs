//! Best-effort thread quality-of-service hints.
//!
//! macOS exposes non-privileged pthread QoS classes. Other platforms leave
//! the request unsupported so their existing scheduler setup remains in
//! control (including Windows MMCSS).

/// A Darwin QoS class or an unknown class reported by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    UserInteractive,
    UserInitiated,
    Default,
    Utility,
    Background,
    Unspecified,
    Other(u32),
}

/// The observed and requested QoS for one thread.
///
/// `set_result` is Darwin's pthread return code: zero means the request was
/// accepted. `before` and `after` contain the class and relative priority
/// reported by pthread; this is not a guarantee of real-time execution.
/// They are `None` on platforms without these QoS APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub before: Option<(Class, i32)>,
    pub requested: Class,
    pub set_result: Option<i32>,
    pub after: Option<(Class, i32)>,
}

impl Report {
    /// Whether pthread reports the requested class after the request.
    pub fn established(self) -> bool {
        self.after.is_some_and(|(class, _)| class == self.requested)
    }
}

/// Requests a non-privileged QoS hint and reports the configured class before
/// and after. Call at worker-thread entry, outside real-time callbacks.
pub fn request(requested: Class) -> Report {
    #[cfg(target_os = "macos")]
    {
        use libc::{
            pthread_get_qos_class_np, pthread_self, pthread_set_qos_class_self_np, qos_class_t,
        };

        fn query() -> Option<(Class, i32)> {
            let mut class = qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut priority = 0;
            // SAFETY: `pthread_self` identifies the calling thread and both
            // pointers remain valid writable values for the duration of the call.
            let result =
                unsafe { pthread_get_qos_class_np(pthread_self(), &mut class, &mut priority) };
            (result == 0).then(|| (from_darwin(class as u32), priority))
        }

        let before = query();
        let set_result = as_darwin(requested).map(|class| {
            // SAFETY: the class is one of Darwin's documented QoS classes;
            // priority zero requests its default relative priority.
            unsafe { pthread_set_qos_class_self_np(class, 0) }
        });
        Report {
            before,
            requested,
            set_result,
            after: query(),
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Report {
            before: None,
            requested,
            set_result: None,
            after: None,
        }
    }
}

#[cfg(target_os = "macos")]
fn as_darwin(class: Class) -> Option<libc::qos_class_t> {
    use libc::qos_class_t;

    Some(match class {
        Class::UserInteractive => qos_class_t::QOS_CLASS_USER_INTERACTIVE,
        Class::UserInitiated => qos_class_t::QOS_CLASS_USER_INITIATED,
        Class::Default => qos_class_t::QOS_CLASS_DEFAULT,
        Class::Utility => qos_class_t::QOS_CLASS_UTILITY,
        Class::Background => qos_class_t::QOS_CLASS_BACKGROUND,
        Class::Unspecified | Class::Other(_) => return None,
    })
}

#[cfg(target_os = "macos")]
fn from_darwin(class: u32) -> Class {
    use libc::qos_class_t;

    match class {
        x if x == qos_class_t::QOS_CLASS_USER_INTERACTIVE as u32 => Class::UserInteractive,
        x if x == qos_class_t::QOS_CLASS_USER_INITIATED as u32 => Class::UserInitiated,
        x if x == qos_class_t::QOS_CLASS_DEFAULT as u32 => Class::Default,
        x if x == qos_class_t::QOS_CLASS_UTILITY as u32 => Class::Utility,
        x if x == qos_class_t::QOS_CLASS_BACKGROUND as u32 => Class::Background,
        x if x == qos_class_t::QOS_CLASS_UNSPECIFIED as u32 => Class::Unspecified,
        other => Class::Other(other),
    }
}
