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
static NSString *const TICKS_KEY = @"ticks";
static NSString *const SAVED_AT_KEY = @"saved_at";
static NSString *const DELETED_KEY = @"deleted";
static const int64_t WAIT_SECONDS = 120;
// A fetch waits for every file in its batch, so a small batch keeps a slow
// link making progress.
static const NSUInteger BATCH_RECORDS = 8;

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
    return @{@"error" : error.localizedDescription ?: @"CloudKit failed"};
}

static BOOL wait_for(dispatch_semaphore_t done) {
    return dispatch_semaphore_wait(
               done, dispatch_time(DISPATCH_TIME_NOW,
                                   WAIT_SECONDS * (int64_t)NSEC_PER_SEC)) == 0;
}

// Runs an operation at a priority the system does not defer, and waits for it.
// One that does not answer in time is cancelled.
static BOOL perform(CKDatabase *database, CKDatabaseOperation *operation,
                    dispatch_semaphore_t done) {
    operation.qualityOfService = NSQualityOfServiceUserInitiated;
    [database addOperation:operation];
    if (wait_for(done)) {
        return YES;
    }
    [operation cancel];
    return NO;
}

static NSError *no_answer(void) {
    return [NSError errorWithDomain:CKErrorDomain
                               code:CKErrorNetworkFailure
                           userInfo:@{
                               NSLocalizedDescriptionKey :
                                   @"CloudKit did not answer in time"
                           }];
}

// Whether `error`, or any item's error inside a partial failure, has `code`.
static BOOL has_code(NSError *error, CKErrorCode code) {
    if (error == nil) {
        return NO;
    }
    if (error.code == code) {
        return YES;
    }
    if (error.code != CKErrorPartialFailure) {
        return NO;
    }
    NSDictionary *partial = error.userInfo[CKPartialErrorsByItemIDKey];
    for (NSError *each in partial.allValues) {
        if (each.code == code) {
            return YES;
        }
    }
    return NO;
}

// Whether the zone is gone, as in an account the player switched to since
// this process made it.
static BOOL zone_missing(NSError *error) {
    return has_code(error, CKErrorZoneNotFound) ||
           has_code(error, CKErrorUserDeletedZone);
}

// Whether the zone is known to exist. Only the mirror's thread calls in.
static BOOL zone_ready = NO;

// Creates the zone unless it is known to exist; saving an existing zone
// succeeds.
static NSError *ensure_zone(CKDatabase *database) {
    if (zone_ready) {
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
    if (!perform(database, operation, done)) {
        return no_answer();
    }
    zone_ready = failed == nil;
    return failed;
}

// {"account": "<user record name>"} while an iCloud account can use
// CloudKit, otherwise {"unavailable": "<reason>"}.
static char *current_account(void) {
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

// One fetch of the zone's changes since `previous`, each changed record's
// file copied into `folder`: the answer, or nil with `*failed` naming the
// error.
static NSDictionary *fetch_once(CKDatabase *database,
                                CKServerChangeToken *previous,
                                NSString *folder, NSError **failed_out) {
    NSMutableArray *changed = [NSMutableArray array];
    __block NSUInteger missed = 0;
    __block NSString *next = nil;
    __block BOOL more = NO;
    __block NSError *failed = nil;
    CKFetchRecordZoneChangesConfiguration *configuration =
        [[CKFetchRecordZoneChangesConfiguration alloc] init];
    configuration.previousServerChangeToken = previous;
    configuration.resultsLimit = BATCH_RECORDS;
    CKFetchRecordZoneChangesOperation *operation =
        [[CKFetchRecordZoneChangesOperation alloc]
            initWithRecordZoneIDs:@[ zone_id() ]
            configurationsByRecordZoneID:@{zone_id() : configuration}];
    operation.fetchAllChanges = NO;
    operation.recordWasChangedBlock = ^(CKRecordID *record_id,
                                        CKRecord *record, NSError *error) {
      // A record that could not be fetched or copied is counted, so the
      // caller keeps its token and fetches the record again.
      if (record == nil || error != nil) {
          missed++;
          return;
      }
      NSNumber *ticks = record[TICKS_KEY];
      NSNumber *saved_at = record[SAVED_AT_KEY];
      NSNumber *is_deleted = record[DELETED_KEY];
      if (is_deleted.boolValue) {
          [changed addObject:@{
              @"name" : record_id.recordName,
              @"ticks" : ticks ?: @0,
              @"saved_at" : saved_at ?: @0,
          }];
          return;
      }
      // A record with neither a file nor a delete mark is nothing to apply.
      CKAsset *asset = record[FILE_KEY];
      if (![asset isKindOfClass:CKAsset.class] || asset.fileURL == nil) {
          return;
      }
      NSString *copy =
          [folder stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
      if (![NSFileManager.defaultManager
              copyItemAtURL:asset.fileURL
                      toURL:[NSURL fileURLWithPath:copy]
                      error:nil]) {
          missed++;
          return;
      }
      [changed addObject:@{
          @"name" : record_id.recordName,
          @"ticks" : ticks ?: @0,
          @"saved_at" : saved_at ?: @0,
          @"staged" : copy,
      }];
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
              NSData *data =
                  [NSKeyedArchiver archivedDataWithRootObject:server
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
    if (!perform(database, operation, done)) {
        *failed_out = no_answer();
        return nil;
    }
    if (failed != nil) {
        *failed_out = failed;
        return nil;
    }
    return @{
        @"changed" : changed,
        @"token" : next ?: NSNull.null,
        @"more" : @(more),
        @"failed" : @(missed),
    };
}

char *aftermine_icloud_cloudkit_account(void) {
    @autoreleasepool {
        @try {
            return current_account();
        } @catch (NSException *exception) {
            return json(@{@"unavailable" : exception.reason ?: @"CloudKit failed"});
        }
    }
}

// Every change to the zone since `token` (base64, or NULL for all of it).
// Each changed record's file is copied into `staging`, since CloudKit may
// delete its own copy at any time.
static char *fetch_changes(const char *token, const char *staging) {
    CKContainer *app = container();
    if (app == nil) {
        return json(@{@"error" : @"no CloudKit container"});
    }
    CKDatabase *database = app.privateCloudDatabase;
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
    BOOL restarted = token != NULL && previous == nil;
    BOOL remade = NO;
    for (;;) {
        NSError *zone = ensure_zone(database);
        if (zone != nil) {
            return json(failure(zone));
        }
        NSError *failed = nil;
        NSDictionary *answer =
            fetch_once(database, previous, folder, &failed);
        if (answer != nil) {
            NSMutableDictionary *reply = [answer mutableCopy];
            reply[@"restarted"] = @(restarted);
            return json(reply);
        }
        // A zone that is gone is made again, empty, so nothing the
        // caller last agreed with it still holds.
        if (zone_missing(failed) && !remade) {
            zone_ready = NO;
            remade = YES;
            restarted = restarted || previous != nil;
            previous = nil;
            continue;
        }
        // A token CloudKit no longer honors starts the fetch over.
        if (has_code(failed, CKErrorChangeTokenExpired) && previous != nil) {
            restarted = YES;
            previous = nil;
            continue;
        }
        return json(failure(failed));
    }
}

char *aftermine_icloud_cloudkit_fetch(const char *token, const char *staging) {
    @autoreleasepool {
        @try {
            return fetch_changes(token, staging);
        } @catch (NSException *exception) {
            return json(@{@"error" : exception.reason ?: @"CloudKit failed"});
        }
    }
}

// The record CloudKit holds under `record_id`, without its file, or nil with
// `*failed` set unless there simply is none.
static CKRecord *current_record(CKDatabase *database, CKRecordID *record_id,
                                NSError **failed_out) {
    __block CKRecord *found = nil;
    __block NSError *failed = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    CKFetchRecordsOperation *operation =
        [[CKFetchRecordsOperation alloc] initWithRecordIDs:@[ record_id ]];
    operation.desiredKeys = @[ TICKS_KEY, SAVED_AT_KEY, DELETED_KEY ];
    operation.fetchRecordsCompletionBlock =
        ^(NSDictionary<CKRecordID *, CKRecord *> *records, NSError *error) {
          found = records[record_id];
          if (found == nil && !has_code(error, CKErrorUnknownItem)) {
              failed = error;
          }
          dispatch_semaphore_signal(done);
        };
    if (!perform(database, operation, done)) {
        *failed_out = no_answer();
        return nil;
    }
    *failed_out = failed;
    return found;
}

// One save: nil once it is done, or the error, or a timeout. The default
// save policy refuses it if the record changed since it was fetched.
static NSError *save_once(CKDatabase *database, CKRecord *record) {
    __block NSError *failed = nil;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    CKModifyRecordsOperation *operation =
        [[CKModifyRecordsOperation alloc] initWithRecordsToSave:@[ record ]
                                              recordIDsToDelete:nil];
    operation.modifyRecordsCompletionBlock =
        ^(NSArray<CKRecord *> *saved, NSArray<CKRecordID *> *deleted,
          NSError *error) {
          (void)saved;
          (void)deleted;
          failed = error;
          dispatch_semaphore_signal(done);
        };
    return perform(database, operation, done) ? failed : no_answer();
}

// Orders two versions as the mirror does: most ticks, then a file over a
// delete record, then the later save.
static int compare_versions(long long left_ticks, BOOL left_live,
                            long long left_saved, long long right_ticks,
                            BOOL right_live, long long right_saved) {
    if (left_ticks != right_ticks) {
        return left_ticks > right_ticks ? 1 : -1;
    }
    if (left_live != right_live) {
        return left_live ? 1 : -1;
    }
    if (left_saved != right_saved) {
        return left_saved > right_saved ? 1 : -1;
    }
    return 0;
}

// Saves a version of the record `name`: the file at `path`, or with no path
// a delete record. CloudKit only ever moves forward: a version behind the one
// it holds is refused, however late it arrives, and the device learns of the
// one further along on its next fetch.
static char *save_version(const char *name, long long ticks,
                          long long saved_at, NSURL *file) {
    CKContainer *app = container();
    if (app == nil) {
        return json(@{@"error" : @"no CloudKit container"});
    }
    CKDatabase *database = app.privateCloudDatabase;
    CKRecordID *record_id = [[CKRecordID alloc]
        initWithRecordName:[NSString stringWithUTF8String:name]
                    zoneID:zone_id()];
    BOOL live = file != nil;
    BOOL remade = NO;
    for (;;) {
        NSError *zone = ensure_zone(database);
        if (zone != nil) {
            return json(failure(zone));
        }
        NSError *failed = nil;
        CKRecord *record = current_record(database, record_id, &failed);
        if (failed == nil && record != nil) {
            int order = compare_versions(
                [record[TICKS_KEY] longLongValue],
                ![record[DELETED_KEY] boolValue],
                [record[SAVED_AT_KEY] longLongValue], ticks, live, saved_at);
            if (order == 0) {
                return json(@{});
            }
            if (order > 0) {
                return json(@{
                    @"error" : @"CloudKit holds a version further along",
                    @"behind" : @YES,
                });
            }
        }
        if (failed == nil) {
            if (record == nil) {
                record = [[CKRecord alloc] initWithRecordType:RECORD_TYPE
                                                     recordID:record_id];
            }
            record[TICKS_KEY] = @(ticks);
            record[SAVED_AT_KEY] = @(saved_at);
            record[DELETED_KEY] = @(!live);
            record[FILE_KEY] =
                live ? [[CKAsset alloc] initWithFileURL:file] : nil;
            failed = save_once(database, record);
        }
        if (failed == nil) {
            return json(@{});
        }
        if (zone_missing(failed) && !remade) {
            zone_ready = NO;
            remade = YES;
            continue;
        }
        return json(failure(failed));
    }
}

// Saves the file at `path` as the record `name`.
char *aftermine_icloud_cloudkit_upload(const char *name, const char *path,
                                       long long ticks, long long saved_at) {
    @autoreleasepool {
        @try {
            NSURL *file =
                [NSURL fileURLWithPath:[NSString stringWithUTF8String:path]];
            return save_version(name, ticks, saved_at, file);
        } @catch (NSException *exception) {
            return json(@{@"error" : exception.reason ?: @"CloudKit failed"});
        }
    }
}

// Replaces the record `name` with a delete record of tick `ticks`: a record
// rather than an absence, so every device can weigh it against the copy it
// holds, and one that fetches from the start still learns of it.
char *aftermine_icloud_cloudkit_delete(const char *name, long long ticks) {
    @autoreleasepool {
        @try {
            return save_version(name, ticks, 0, nil);
        } @catch (NSException *exception) {
            return json(@{@"error" : exception.reason ?: @"CloudKit failed"});
        }
    }
}
