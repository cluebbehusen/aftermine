// Foundation calls behind the crate's Rust API. Each one may wait on iCloud,
// so callers keep them off the main thread.

#import <Foundation/Foundation.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

typedef void (*aftermine_icloud_accessor)(void *context, const char *path);

static char *copy_path(NSURL *url) {
    return strdup(url.fileSystemRepresentation);
}

static char *copy_message(NSError *error) {
    const char *text = error.localizedDescription.UTF8String;
    return strdup(text != NULL ? text : "iCloud refused access");
}

char *aftermine_icloud_documents(void) {
    @autoreleasepool {
        NSURL *container =
            [NSFileManager.defaultManager URLForUbiquityContainerIdentifier:nil];
        if (container == nil) {
            return NULL;
        }
        return copy_path([container URLByAppendingPathComponent:@"Documents"
                                                    isDirectory:YES]);
    }
}

// access: 0 reads, 1 replaces, 2 deletes. The accessor receives the path the
// coordinator grants, which differs from `path` when the file moved meanwhile.
// Returns whether the accessor ran; when it did not, *error_out holds a
// message the caller frees.
bool aftermine_icloud_coordinate(const char *path, int access, void *context,
                                 aftermine_icloud_accessor accessor,
                                 char **error_out) {
    @autoreleasepool {
        NSURL *url = [NSURL fileURLWithFileSystemRepresentation:path
                                                    isDirectory:NO
                                                  relativeToURL:nil];
        NSFileCoordinator *coordinator =
            [[NSFileCoordinator alloc] initWithFilePresenter:nil];
        __block bool ran = false;
        void (^body)(NSURL *) = ^(NSURL *granted) {
            ran = true;
            accessor(context, granted.fileSystemRepresentation);
        };
        NSError *error = nil;
        if (access == 0) {
            [coordinator coordinateReadingItemAtURL:url
                                            options:0
                                              error:&error
                                         byAccessor:body];
        } else {
            NSFileCoordinatorWritingOptions options =
                access == 1 ? NSFileCoordinatorWritingForReplacing
                            : NSFileCoordinatorWritingForDeleting;
            [coordinator coordinateWritingItemAtURL:url
                                            options:options
                                              error:&error
                                         byAccessor:body];
        }
        if (!ran) {
            *error_out = copy_message(error);
        }
        return ran;
    }
}

// Keeps the copy iCloud chose when two devices changed a file at once.
static void keep_current_version(NSURL *item) {
    NSFileCoordinator *coordinator =
        [[NSFileCoordinator alloc] initWithFilePresenter:nil];
    [coordinator
        coordinateWritingItemAtURL:item
                           options:0
                             error:NULL
                        byAccessor:^(NSURL *granted) {
                          [NSFileVersion removeOtherVersionsOfItemAtURL:granted
                                                                  error:NULL];
                          for (NSFileVersion *version in [NSFileVersion
                                   unresolvedConflictVersionsOfItemAtURL:
                                       granted]) {
                              version.resolved = YES;
                          }
                        }];
}

long aftermine_icloud_download(const char *dir) {
    @autoreleasepool {
        NSFileManager *files = NSFileManager.defaultManager;
        NSURL *folder = [NSURL fileURLWithFileSystemRepresentation:dir
                                                       isDirectory:YES
                                                     relativeToURL:nil];
        NSArray<NSURLResourceKey> *keys = @[
            NSURLIsUbiquitousItemKey, NSURLUbiquitousItemDownloadingStatusKey,
            NSURLUbiquitousItemHasUnresolvedConflictsKey
        ];
        NSArray<NSURL *> *items =
            [files contentsOfDirectoryAtURL:folder
                 includingPropertiesForKeys:keys
                                    options:0
                                      error:NULL];
        long downloading = 0;
        for (NSURL *item in items) {
            // iOS stands a hidden `.name.icloud` file in for a file that is
            // not on this device yet.
            NSString *name = item.lastPathComponent;
            if ([name hasPrefix:@"."] &&
                [name.pathExtension isEqualToString:@"icloud"]) {
                NSString *real =
                    [[name substringFromIndex:1] stringByDeletingPathExtension];
                NSURL *file = [folder URLByAppendingPathComponent:real
                                                      isDirectory:NO];
                if ([files startDownloadingUbiquitousItemAtURL:file error:NULL]) {
                    downloading++;
                }
                continue;
            }
            NSDictionary<NSURLResourceKey, id> *values =
                [item resourceValuesForKeys:keys error:NULL];
            if (![values[NSURLIsUbiquitousItemKey] boolValue]) {
                continue;
            }
            if ([values[NSURLUbiquitousItemHasUnresolvedConflictsKey] boolValue]) {
                keep_current_version(item);
            }
            NSString *status = values[NSURLUbiquitousItemDownloadingStatusKey];
            if (status != nil &&
                ![status isEqualToString:NSURLUbiquitousItemDownloadingStatusCurrent] &&
                [files startDownloadingUbiquitousItemAtURL:item error:NULL]) {
                downloading++;
            }
        }
        return downloading;
    }
}
