// NSUbiquitousKeyValueStore calls behind the crate's key-value API. The
// store keeps its values in memory and syncs them in the background, so
// these calls do not wait on iCloud. Unlike iCloud Drive, the store exists
// on Apple TV too.

#import <Foundation/Foundation.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

// How many times the store has told this device that its contents changed
// from outside: another device's write, an account change or a refused
// write.
static atomic_long external_changes = 0;

// Starts counting outside changes, once per process, then asks the store to
// catch up with iCloud. Returns false when the store refuses this app, as it
// does for a build without the key-value store entitlement.
bool aftermine_icloud_key_value_open(void) {
    @autoreleasepool {
        NSUbiquitousKeyValueStore *store =
            NSUbiquitousKeyValueStore.defaultStore;
        static dispatch_once_t observing;
        dispatch_once(&observing, ^{
          // The center holds the observer until it is removed, which it
          // never is.
          (void)[NSNotificationCenter.defaultCenter
              addObserverForName:
                  NSUbiquitousKeyValueStoreDidChangeExternallyNotification
                          object:store
                           queue:nil
                      usingBlock:^(NSNotification *note) {
                        (void)note;
                        atomic_fetch_add(&external_changes, 1);
                      }];
        });
        return [store synchronize];
    }
}

// The string stored under `key`, as a copy the caller frees, or NULL when
// there is none.
char *aftermine_icloud_key_value_get(const char *key) {
    @autoreleasepool {
        NSString *name = [NSString stringWithUTF8String:key];
        if (name == nil) {
            return NULL;
        }
        NSString *value =
            [NSUbiquitousKeyValueStore.defaultStore stringForKey:name];
        const char *text = value.UTF8String;
        return text != NULL ? strdup(text) : NULL;
    }
}

void aftermine_icloud_key_value_set(const char *key, const char *value) {
    @autoreleasepool {
        NSString *name = [NSString stringWithUTF8String:key];
        NSString *text = [NSString stringWithUTF8String:value];
        if (name == nil || text == nil) {
            return;
        }
        [NSUbiquitousKeyValueStore.defaultStore setString:text forKey:name];
    }
}

long aftermine_icloud_key_value_changes(void) {
    return atomic_load(&external_changes);
}
