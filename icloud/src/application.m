// UIApplication calls behind the crate's application API: the idle timer and
// background tasks, which only iPad and Apple TV have.

#import <UIKit/UIKit.h>
#include <stdbool.h>
#include <stdint.h>

void aftermine_icloud_keep_screen_awake(bool awake) {
    UIApplication.sharedApplication.idleTimerDisabled = awake;
}

// The tasks begun and not yet ended. Both the system, when a task's time
// runs out, and the task's owner end it, and only the first reaches UIKit.
static NSMutableSet<NSNumber *> *live_tasks(void) {
    static NSMutableSet<NSNumber *> *tasks;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      tasks = [NSMutableSet set];
    });
    return tasks;
}

void aftermine_icloud_end_background_task(uintptr_t task) {
    @autoreleasepool {
        NSMutableSet<NSNumber *> *tasks = live_tasks();
        NSNumber *key = @(task);
        BOOL live;
        @synchronized(tasks) {
            live = [tasks containsObject:key];
            [tasks removeObject:key];
        }
        if (live) {
            [UIApplication.sharedApplication endBackgroundTask:task];
        }
    }
}

// The new task, or UIBackgroundTaskInvalid when the system grants none. It is
// recorded before the lock is let go, so its expiry cannot miss it.
uintptr_t aftermine_icloud_begin_background_task(void) {
    @autoreleasepool {
        NSMutableSet<NSNumber *> *tasks = live_tasks();
        __block UIBackgroundTaskIdentifier task = UIBackgroundTaskInvalid;
        @synchronized(tasks) {
            task = [UIApplication.sharedApplication
                beginBackgroundTaskWithName:@"Saving"
                          expirationHandler:^{
                            aftermine_icloud_end_background_task(task);
                          }];
            if (task != UIBackgroundTaskInvalid) {
                [tasks addObject:@(task)];
            }
        }
        return task;
    }
}
