// CloudKit calls behind the crate's record mirror: one private-database zone
// holding one record per file, each carrying the file as an asset. Every
// call waits for CloudKit to answer, so only a worker thread may make one.
// Results cross to Rust as JSON strings the caller frees.

#import <CloudKit/CloudKit.h>
#import <Foundation/Foundation.h>
#include <libkern/OSByteOrder.h>
#include <mach-o/dyld.h>
#include <mach-o/getsect.h>
#include <mach-o/loader.h>
#include <stdlib.h>
#include <string.h>

static NSString *const ZONE = @"Records";
static NSString *const RECORD_TYPE = @"Record";
static NSString *const FILE_KEY = @"file";
static NSString *const SAVED_AT_KEY = @"saved_at";
static const int64_t WAIT_SECONDS = 120;

// The entitlements blob in a code signature `size` bytes long: the XML
// after the header of the embedded-entitlements blob the signature's index
// points at. All fields are big-endian.
static NSData *entitlements_in(const uint8_t *signature, uint64_t size) {
    if (size < 12 || OSReadBigInt32(signature, 0) != 0xfade0cc0) {
        return nil;
    }
    uint32_t count = OSReadBigInt32(signature, 8);
    for (uint64_t i = 0; i < count && 12 + 8 * (i + 1) <= size; i++) {
        uint64_t offset = OSReadBigInt32(signature, 12 + 8 * i + 4);
        if (offset + 8 > size || OSReadBigInt32(signature, offset) != 0xfade7171) {
            continue;
        }
        uint64_t length = OSReadBigInt32(signature, offset + 4);
        if (length < 8 || offset + length > size) {
            return nil;
        }
        return [NSData dataWithBytes:signature + offset + 8 length:length - 8];
    }
    return nil;
}

// The entitlements this process was signed with: a simulator build keeps
// them in a section of its own, a device build in its code signature, which
// sits mapped at the end of __LINKEDIT.
static NSData *own_entitlements(void) {
    const struct mach_header_64 *header =
        (const struct mach_header_64 *)_dyld_get_image_header(0);
    if (header == NULL || header->magic != MH_MAGIC_64) {
        return nil;
    }
    unsigned long section_size = 0;
    const uint8_t *section =
        getsectiondata(header, "__TEXT", "__entitlements", &section_size);
    if (section != NULL && section_size > 0) {
        return [NSData dataWithBytes:section length:section_size];
    }
    const struct segment_command_64 *linkedit = NULL;
    const struct linkedit_data_command *signature = NULL;
    const uint8_t *command = (const uint8_t *)(header + 1);
    for (uint32_t i = 0; i < header->ncmds; i++) {
        const struct load_command *load = (const struct load_command *)command;
        if (load->cmd == LC_SEGMENT_64 &&
            strcmp(((const struct segment_command_64 *)load)->segname,
                   SEG_LINKEDIT) == 0) {
            linkedit = (const struct segment_command_64 *)load;
        } else if (load->cmd == LC_CODE_SIGNATURE) {
            signature = (const struct linkedit_data_command *)load;
        }
        command += load->cmdsize;
    }
    if (linkedit == NULL || signature == NULL ||
        signature->dataoff < linkedit->fileoff ||
        signature->dataoff + signature->datasize >
            linkedit->fileoff + linkedit->filesize) {
        return nil;
    }
    uintptr_t start = (uintptr_t)linkedit->vmaddr +
                      (uintptr_t)_dyld_get_image_vmaddr_slide(0) +
                      (signature->dataoff - (uintptr_t)linkedit->fileoff);
    return entitlements_in((const uint8_t *)start, signature->datasize);
}

// Whether this build may use CloudKit. CloudKit stops a process that asks
// for a container without the entitlement, so nothing touches it otherwise.
static BOOL entitled(void) {
    NSData *data = own_entitlements();
    if (data == nil) {
        return NO;
    }
    NSDictionary *entitlements =
        [NSPropertyListSerialization propertyListWithData:data
                                                  options:0
                                                   format:nil
                                                    error:nil];
    if (![entitlements isKindOfClass:NSDictionary.class]) {
        return NO;
    }
    NSArray *services = entitlements[@"com.apple.developer.icloud-services"];
    return [services isKindOfClass:NSArray.class] &&
           [services containsObject:@"CloudKit"];
}

// The app's container, `iCloud.<bundle ID>`, or nil when there is no bundle
// (a test binary) or the build lacks the CloudKit entitlement.
static CKContainer *container(void) {
    static CKContainer *shared = nil;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      NSString *bundle = NSBundle.mainBundle.bundleIdentifier;
      if (bundle == nil || !entitled()) {
          return;
      }
      @try {
          shared = [CKContainer
              containerWithIdentifier:[@"iCloud."
                                          stringByAppendingString:bundle]];
      } @catch (NSException *exception) {
          (void)exception;
          shared = nil;
      }
    });
    return shared;
}

static CKRecordZoneID *zone_id(void) {
    return [[CKRecordZoneID alloc] initWithZoneName:ZONE
                                          ownerName:CKCurrentUserDefaultName];
}

static char *json(NSDictionary *object) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:object
                                                   options:0
                                                     error:nil];
    if (data == nil) {
        return NULL;
    }
    NSString *text = [[NSString alloc] initWithData:data
                                           encoding:NSUTF8StringEncoding];
    return text != nil ? strdup(text.UTF8String) : NULL;
}

static NSDictionary *failure(NSError *error) {
    return @{
        @"error" : error.localizedDescription ?: @"CloudKit failed",
        @"code" : @(error.code),
    };
}

static BOOL wait_for(dispatch_semaphore_t done) {
    return dispatch_semaphore_wait(
               done, dispatch_time(DISPATCH_TIME_NOW,
                                   WAIT_SECONDS * (int64_t)NSEC_PER_SEC)) == 0;
}

static NSDictionary *timed_out(void) {
    return @{@"error" : @"CloudKit did not answer in time", @"code" : @(-1)};
}

// Creates the zone once per process; saving an existing zone succeeds.
static NSError *ensure_zone(CKDatabase *database) {
    static BOOL ready = NO;
    if (ready) {
        return nil;
    }
    __block NSError *failed = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    CKModifyRecordZonesOperation *operation =
        [[CKModifyRecordZonesOperation alloc]
            initWithRecordZonesToSave:@[ [[CKRecordZone alloc]
                                          initWithZoneID:zone_id()] ]
                recordZoneIDsToDelete:nil];
    operation.modifyRecordZonesCompletionBlock =
        ^(NSArray<CKRecordZone *> *saved, NSArray<CKRecordZoneID *> *deleted,
          NSError *error) {
          (void)saved;
          (void)deleted;
          failed = error;
          dispatch_semaphore_signal(done);
        };
    [database addOperation:operation];
    if (!wait_for(done)) {
        return [NSError errorWithDomain:CKErrorDomain
                                   code:CKErrorNetworkFailure
                               userInfo:nil];
    }
    ready = failed == nil;
    return failed;
}

// {"account": "<user record name>"} while an iCloud account can use
// CloudKit, otherwise {"unavailable": "<reason>"}.
char *aftermine_icloud_cloudkit_account(void) {
    @autoreleasepool {
        CKContainer *app = container();
        if (app == nil) {
            return json(@{@"unavailable" : @"no CloudKit container"});
        }
        __block NSDictionary *answer = nil;
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        [app accountStatusWithCompletionHandler:^(CKAccountStatus status,
                                                  NSError *error) {
          if (status != CKAccountStatusAvailable) {
              answer = @{
                  @"unavailable" : error.localizedDescription
                      ?: [NSString stringWithFormat:@"account status %ld",
                                                    (long)status]
              };
              dispatch_semaphore_signal(done);
              return;
          }
          [app fetchUserRecordIDWithCompletionHandler:^(CKRecordID *record,
                                                        NSError *failed) {
            answer = record != nil
                         ? @{@"account" : record.recordName}
                         : @{
                               @"unavailable" : failed.localizedDescription
                                   ?: @"no user record"
                           };
            dispatch_semaphore_signal(done);
          }];
        }];
        if (!wait_for(done)) {
            return json(@{@"unavailable" : @"CloudKit did not answer in time"});
        }
        return json(answer);
    }
}

// Every change to the zone since `token` (base64, or NULL for all of it).
// Each changed record's file is copied into `staging`, since CloudKit may
// delete its own copy at any time.
char *aftermine_icloud_cloudkit_fetch(const char *token, const char *staging) {
    @autoreleasepool {
        CKContainer *app = container();
        if (app == nil) {
            return json(@{@"error" : @"no CloudKit container", @"code" : @(-2)});
        }
        CKDatabase *database = app.privateCloudDatabase;
        NSError *zone = ensure_zone(database);
        if (zone != nil) {
            return json(failure(zone));
        }
        CKServerChangeToken *previous = nil;
        if (token != NULL) {
            NSData *data = [[NSData alloc]
                initWithBase64EncodedString:[NSString
                                                stringWithUTF8String:token]
                                    options:0];
            if (data != nil) {
                previous = [NSKeyedUnarchiver
                    unarchivedObjectOfClass:CKServerChangeToken.class
                                   fromData:data
                                      error:nil];
            }
        }
        NSString *folder = [NSString stringWithUTF8String:staging];
        NSMutableArray *changed = [NSMutableArray array];
        NSMutableArray *deleted = [NSMutableArray array];
        __block NSString *next = nil;
        __block BOOL more = NO;
        __block NSError *failed = nil;
        CKFetchRecordZoneChangesConfiguration *configuration =
            [[CKFetchRecordZoneChangesConfiguration alloc] init];
        configuration.previousServerChangeToken = previous;
        CKFetchRecordZoneChangesOperation *operation =
            [[CKFetchRecordZoneChangesOperation alloc]
                initWithRecordZoneIDs:@[ zone_id() ]
                configurationsByRecordZoneID:@{zone_id() : configuration}];
        operation.fetchAllChanges = NO;
        operation.recordWasChangedBlock = ^(CKRecordID *record_id,
                                            CKRecord *record, NSError *error) {
          if (record == nil || error != nil) {
              return;
          }
          CKAsset *asset = record[FILE_KEY];
          NSNumber *saved_at = record[SAVED_AT_KEY];
          if (![asset isKindOfClass:CKAsset.class] || asset.fileURL == nil) {
              return;
          }
          NSString *copy = [folder
              stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
          if (![NSFileManager.defaultManager
                  copyItemAtURL:asset.fileURL
                          toURL:[NSURL fileURLWithPath:copy]
                          error:nil]) {
              return;
          }
          [changed addObject:@{
              @"name" : record_id.recordName,
              @"saved_at" : saved_at ?: @0,
              @"staged" : copy,
          }];
        };
        operation.recordWithIDWasDeletedBlock =
            ^(CKRecordID *record_id, CKRecordType type) {
              (void)type;
              [deleted addObject:record_id.recordName];
            };
        operation.recordZoneFetchCompletionBlock =
            ^(CKRecordZoneID *zone_id, CKServerChangeToken *server,
              NSData *client, BOOL more_coming, NSError *error) {
              (void)zone_id;
              (void)client;
              if (error != nil) {
                  failed = error;
                  return;
              }
              more = more_coming;
              if (server != nil) {
                  NSData *data = [NSKeyedArchiver
                      archivedDataWithRootObject:server
                           requiringSecureCoding:YES
                                           error:nil];
                  next = [data base64EncodedStringWithOptions:0];
              }
            };
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        operation.fetchRecordZoneChangesCompletionBlock = ^(NSError *error) {
          if (failed == nil) {
              failed = error;
          }
          dispatch_semaphore_signal(done);
        };
        [database addOperation:operation];
        if (!wait_for(done)) {
            return json(timed_out());
        }
        if (failed != nil) {
            return json(failure(failed));
        }
        return json(@{
            @"changed" : changed,
            @"deleted" : deleted,
            @"token" : next ?: NSNull.null,
            @"more" : @(more),
        });
    }
}

static char *modify(NSArray<CKRecord *> *save, NSArray<CKRecordID *> *remove) {
    CKContainer *app = container();
    if (app == nil) {
        return json(@{@"error" : @"no CloudKit container", @"code" : @(-2)});
    }
    CKDatabase *database = app.privateCloudDatabase;
    NSError *zone = ensure_zone(database);
    if (zone != nil) {
        return json(failure(zone));
    }
    __block NSError *failed = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    CKModifyRecordsOperation *operation =
        [[CKModifyRecordsOperation alloc] initWithRecordsToSave:save
                                              recordIDsToDelete:remove];
    // The mirror decides which copy wins before it uploads, so the upload
    // replaces whatever the server holds.
    operation.savePolicy = CKRecordSaveAllKeys;
    operation.modifyRecordsCompletionBlock =
        ^(NSArray<CKRecord *> *saved, NSArray<CKRecordID *> *deleted,
          NSError *error) {
          (void)saved;
          (void)deleted;
          failed = error;
          dispatch_semaphore_signal(done);
        };
    [database addOperation:operation];
    if (!wait_for(done)) {
        return json(timed_out());
    }
    // Deleting a record that is already gone is a success.
    if (failed != nil && failed.code == CKErrorPartialFailure) {
        NSDictionary *partial = failed.userInfo[CKPartialErrorsByItemIDKey];
        BOOL only_missing = partial.count > 0;
        for (NSError *each in partial.allValues) {
            only_missing = only_missing && each.code == CKErrorUnknownItem;
        }
        if (only_missing) {
            failed = nil;
        }
    }
    return failed != nil ? json(failure(failed)) : json(@{});
}

// Saves the file at `path` as the record `name`.
char *aftermine_icloud_cloudkit_upload(const char *name, const char *path,
                                       long long saved_at, long long size) {
    @autoreleasepool {
        CKRecordID *record_id = [[CKRecordID alloc]
            initWithRecordName:[NSString stringWithUTF8String:name]
                        zoneID:zone_id()];
        CKRecord *record = [[CKRecord alloc] initWithRecordType:RECORD_TYPE
                                                       recordID:record_id];
        record[FILE_KEY] = [[CKAsset alloc]
            initWithFileURL:[NSURL fileURLWithPath:
                                       [NSString stringWithUTF8String:path]]];
        record[SAVED_AT_KEY] = @(saved_at);
        record[@"size"] = @(size);
        return modify(@[ record ], nil);
    }
}

// Deletes the record `name`; one already gone counts as deleted.
char *aftermine_icloud_cloudkit_delete(const char *name) {
    @autoreleasepool {
        CKRecordID *record_id = [[CKRecordID alloc]
            initWithRecordName:[NSString stringWithUTF8String:name]
                        zoneID:zone_id()];
        return modify(nil, @[ record_id ]);
    }
}
