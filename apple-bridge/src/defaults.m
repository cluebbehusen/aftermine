// NSUserDefaults calls behind the crate's user defaults API. Apple TV keeps
// only user defaults when it frees space, so the settings that must last
// there live here rather than in a file.

#import <Foundation/Foundation.h>
#include <stdlib.h>
#include <string.h>

// The string stored under `key`, as a copy the caller frees, or NULL when
// there is none.
char *aftermine_icloud_defaults_get(const char *key) {
    @autoreleasepool {
        NSString *name = [NSString stringWithUTF8String:key];
        if (name == nil) {
            return NULL;
        }
        NSString *value =
            [NSUserDefaults.standardUserDefaults stringForKey:name];
        const char *text = value.UTF8String;
        return text != NULL ? strdup(text) : NULL;
    }
}

void aftermine_icloud_defaults_set(const char *key, const char *value) {
    @autoreleasepool {
        NSString *name = [NSString stringWithUTF8String:key];
        NSString *text = [NSString stringWithUTF8String:value];
        if (name == nil || text == nil) {
            return;
        }
        [NSUserDefaults.standardUserDefaults setObject:text forKey:name];
    }
}
