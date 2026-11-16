use std::cell::OnceCell;

use block2::{DynBlock, RcBlock};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
    AnyThread,
};
use objc2_foundation::{
    ns_string, NSBundle, NSError, NSObject, NSObjectProtocol, NSProcessInfo, NSString,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

define_class!(
    // NSObject 没有额外的子类约束，通知代理无可变状态。
    #[unsafe(super = NSObject)]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present_notification(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            // 即使 HostGuard 正打开，也按系统通知设置显示提示。
            let options = if NSProcessInfo::processInfo()
                .operatingSystemVersion()
                .majorVersion
                >= 11
            {
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List
            } else {
                #[allow(deprecated)]
                UNNotificationPresentationOptions::Alert
            };
            completion.call((options,));
        }
    }
);

thread_local! {
    // UNUserNotificationCenter 弱引用代理，需在主线程保留。
    static DELEGATE: OnceCell<Retained<NotificationDelegate>> = const { OnceCell::new() };
}

pub(crate) fn show_cpu_alert(message: String) {
    // 终端直接启动调试二进制时没有应用包，系统通知要求可识别的应用包。
    if NSBundle::mainBundle()
        .bundleURL()
        .pathExtension()
        .as_deref()
        != Some(ns_string!("app"))
    {
        return;
    }

    let center = UNUserNotificationCenter::currentNotificationCenter();
    DELEGATE.with(|cell| {
        let delegate = cell.get_or_init(|| {
            // SAFETY: 调用 NSObject 的标准初始化方法，代理没有自定义字段。
            unsafe { msg_send![NotificationDelegate::alloc(), init] }
        });
        center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
    });

    // 首次告警才申请通知权限；macOS 会记住用户的选择。
    let permission = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if granted.as_bool() {
            let content = UNMutableNotificationContent::new();
            content.setTitle(ns_string!("HostGuard"));
            content.setBody(&NSString::from_str(&message));
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                ns_string!("hostguard-cpu-alert"),
                &content,
                None,
            );
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, None);
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert,
        &permission,
    );
}
