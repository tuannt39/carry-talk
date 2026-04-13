#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <AudioToolbox/AudioToolbox.h>
#import <stdatomic.h>

typedef void (*carrytalk_audio_callback_t)(const float *samples,
                                           size_t sample_count,
                                           uint32_t sample_rate,
                                           uint16_t channels,
                                           void *user_data);

typedef struct carrytalk_macos_sc_stream_handle carrytalk_macos_sc_stream_handle;

@interface CarryTalkStreamOutput : NSObject <SCStreamOutput>
@property(nonatomic, assign) carrytalk_macos_sc_stream_handle *handle;
@end

struct carrytalk_macos_sc_stream_handle {
    uint32_t display_id;
    _Atomic(uint32_t) sample_rate;
    _Atomic(uint16_t) channels;
    carrytalk_audio_callback_t callback;
    void *user_data;
    _Atomic(bool) running;
    char last_error[512];
    SCStream *stream;
    SCContentFilter *filter;
    SCStreamConfiguration *configuration;
    dispatch_queue_t queue;
    CarryTalkStreamOutput *output;
};

static void carrytalk_set_error(carrytalk_macos_sc_stream_handle *handle, NSString *message) {
    if (handle == NULL) {
        return;
    }
    NSString *resolved_message = message != nil ? message : @"Unknown macOS ScreenCaptureKit error";
    const char *utf8 = [resolved_message UTF8String];
    if (utf8 == NULL) {
        utf8 = "Unknown macOS ScreenCaptureKit error";
    }
    snprintf(handle->last_error, sizeof(handle->last_error), "%s", utf8);
}

static SCDisplay *carrytalk_find_display(NSArray<SCDisplay *> *displays, uint32_t display_id) API_AVAILABLE(macos(13.0)) {
    for (SCDisplay *display in displays) {
        if ((uint32_t)display.displayID == display_id) {
            return display;
        }
    }
    return nil;
}

static BOOL carrytalk_extract_and_emit_audio(carrytalk_macos_sc_stream_handle *handle, CMSampleBufferRef sample_buffer) API_AVAILABLE(macos(13.0)) {
    if (handle == NULL || sample_buffer == NULL || handle->callback == NULL) {
        return YES;
    }
    if (!CMSampleBufferIsValid(sample_buffer) || !CMSampleBufferDataIsReady(sample_buffer)) {
        return YES;
    }

    CMAudioFormatDescriptionRef format_description = CMSampleBufferGetFormatDescription(sample_buffer);
    const AudioStreamBasicDescription *asbd = CMAudioFormatDescriptionGetStreamBasicDescription(format_description);
    if (asbd == NULL) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered audio without ASBD metadata");
        return NO;
    }
    if (asbd->mFormatID != kAudioFormatLinearPCM) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered non-LPCM audio format");
        return NO;
    }
    if ((asbd->mFormatFlags & kAudioFormatFlagIsFloat) == 0) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered non-float PCM audio");
        return NO;
    }
    if ((asbd->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered non-interleaved audio; backend expects interleaved float PCM");
        return NO;
    }
    if (asbd->mBitsPerChannel != 32) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered unexpected PCM bit depth");
        return NO;
    }

    uint32_t expected_sample_rate = atomic_load(&handle->sample_rate);
    uint16_t expected_channels = atomic_load(&handle->channels);
    if ((uint32_t)asbd->mSampleRate != expected_sample_rate || (uint16_t)asbd->mChannelsPerFrame != expected_channels) {
        carrytalk_set_error(handle, [NSString stringWithFormat:@"ScreenCaptureKit delivered unexpected audio format %u Hz / %u ch (expected %u Hz / %u ch)", (uint32_t)asbd->mSampleRate, (uint16_t)asbd->mChannelsPerFrame, expected_sample_rate, expected_channels]);
        return NO;
    }

    CMBlockBufferRef data_buffer = CMSampleBufferGetDataBuffer(sample_buffer);
    if (data_buffer == NULL) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered audio sample buffer without data block");
        return NO;
    }

    size_t total_bytes = (size_t)CMBlockBufferGetDataLength(data_buffer);
    if (total_bytes == 0) {
        return YES;
    }
    if ((total_bytes % sizeof(float)) != 0) {
        carrytalk_set_error(handle, @"ScreenCaptureKit delivered misaligned float PCM buffer");
        return NO;
    }

    float *interleaved = malloc(total_bytes);
    if (interleaved == NULL) {
        carrytalk_set_error(handle, @"Failed to allocate ScreenCaptureKit audio copy buffer");
        return NO;
    }

    OSStatus status = CMBlockBufferCopyDataBytes(data_buffer, 0, total_bytes, interleaved);
    if (status != noErr) {
        free(interleaved);
        carrytalk_set_error(handle, [NSString stringWithFormat:@"Failed to copy ScreenCaptureKit audio bytes: %d", (int)status]);
        return NO;
    }

    size_t total_floats = total_bytes / sizeof(float);
    handle->callback(interleaved, total_floats, expected_sample_rate, expected_channels, handle->user_data);
    free(interleaved);
    return YES;
}

@implementation CarryTalkStreamOutput
- (void)stream:(SCStream *)stream
 didOutputSampleBuffer:(CMSampleBufferRef)sampleBuffer
        ofType:(SCStreamOutputType)type API_AVAILABLE(macos(13.0)) {
    if (type != SCStreamOutputTypeAudio || self.handle == NULL) {
        return;
    }
    if (!carrytalk_extract_and_emit_audio(self.handle, sampleBuffer)) {
        atomic_store(&self.handle->running, false);
    }
}
@end

carrytalk_macos_sc_stream_handle *carrytalk_macos_sc_create_stream(uint32_t display_id,
                                                                   carrytalk_audio_callback_t callback,
                                                                   void *user_data) {
    carrytalk_macos_sc_stream_handle *handle = calloc(1, sizeof(carrytalk_macos_sc_stream_handle));
    if (handle == NULL) {
        return NULL;
    }
    handle->display_id = display_id;
    atomic_store(&handle->sample_rate, 48000);
    atomic_store(&handle->channels, 2);
    handle->callback = callback;
    handle->user_data = user_data;
    atomic_store(&handle->running, false);
    handle->last_error[0] = '\0';
    return handle;
}

bool carrytalk_macos_sc_start_stream(carrytalk_macos_sc_stream_handle *handle) {
    if (handle == NULL) {
        return false;
    }

    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
    __block BOOL started = NO;
    __block NSString *startup_error = nil;

    [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent *content, NSError *error) {
        if (error != nil) {
            startup_error = [NSString stringWithFormat:@"Failed to query ScreenCaptureKit shareable content: %@", error.localizedDescription ?: @"unknown error"];
            dispatch_semaphore_signal(semaphore);
            return;
        }

        SCDisplay *display = carrytalk_find_display(content.displays, handle->display_id);
        if (display == nil) {
            startup_error = [NSString stringWithFormat:@"Requested ScreenCaptureKit display %u is no longer available", handle->display_id];
            dispatch_semaphore_signal(semaphore);
            return;
        }

        handle->filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
        handle->configuration = [SCStreamConfiguration new];
        handle->configuration.capturesAudio = YES;
        handle->configuration.sampleRate = atomic_load(&handle->sample_rate);
        handle->configuration.channelCount = atomic_load(&handle->channels);
        handle->configuration.queueDepth = 8;
        if ([handle->configuration respondsToSelector:@selector(setWidth:)]) {
            handle->configuration.width = 2;
            handle->configuration.height = 2;
        }

        handle->stream = [[SCStream alloc] initWithFilter:handle->filter configuration:handle->configuration delegate:nil];
        handle->queue = dispatch_queue_create("carrytalk.macos.system-audio", DISPATCH_QUEUE_SERIAL);
        handle->output = [CarryTalkStreamOutput new];
        handle->output.handle = handle;

        NSError *add_output_error = nil;
        BOOL added = [handle->stream addStreamOutput:handle->output type:SCStreamOutputTypeAudio sampleHandlerQueue:handle->queue error:&add_output_error];
        if (!added || add_output_error != nil) {
            startup_error = [NSString stringWithFormat:@"Failed to attach ScreenCaptureKit audio output: %@", add_output_error.localizedDescription ?: @"unknown error"];
            dispatch_semaphore_signal(semaphore);
            return;
        }

        [handle->stream startCaptureWithCompletionHandler:^(NSError *start_error) {
            if (start_error != nil) {
                startup_error = [NSString stringWithFormat:@"Failed to start ScreenCaptureKit capture: %@", start_error.localizedDescription ?: @"unknown error"];
            } else {
                atomic_store(&handle->running, true);
                handle->last_error[0] = '\0';
                started = YES;
            }
            dispatch_semaphore_signal(semaphore);
        }];
    }];

    dispatch_semaphore_wait(semaphore, DISPATCH_TIME_FOREVER);

    if (!started) {
        carrytalk_set_error(handle, startup_error ?: @"Failed to start ScreenCaptureKit stream");
        return false;
    }
    return true;
}

void carrytalk_macos_sc_stop_stream(carrytalk_macos_sc_stream_handle *handle) {
    if (handle == NULL) {
        return;
    }

    atomic_store(&handle->running, false);
    if (handle->stream == nil) {
        return;
    }

    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);
    [handle->stream stopCaptureWithCompletionHandler:^(__unused NSError *error) {
        dispatch_semaphore_signal(semaphore);
    }];
    dispatch_semaphore_wait(semaphore, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(2 * NSEC_PER_SEC)));

    if (handle->output != nil) {
        handle->output.handle = NULL;
        NSError *remove_error = nil;
        [handle->stream removeStreamOutput:handle->output type:SCStreamOutputTypeAudio error:&remove_error];
        (void)remove_error;
    }
}

void carrytalk_macos_sc_destroy_stream(carrytalk_macos_sc_stream_handle *handle) {
    if (handle == NULL) {
        return;
    }
    if (handle->output != nil) {
        handle->output.handle = NULL;
    }
    handle->output = nil;
    handle->stream = nil;
    handle->filter = nil;
    handle->configuration = nil;
    handle->queue = NULL;
    free(handle);
}

bool carrytalk_macos_sc_stream_running(const carrytalk_macos_sc_stream_handle *handle) {
    return handle != NULL && atomic_load(&handle->running);
}

uint32_t carrytalk_macos_sc_stream_sample_rate(const carrytalk_macos_sc_stream_handle *handle) {
    return handle != NULL ? atomic_load(&handle->sample_rate) : 0;
}

uint16_t carrytalk_macos_sc_stream_channels(const carrytalk_macos_sc_stream_handle *handle) {
    return handle != NULL ? atomic_load(&handle->channels) : 0;
}

const char *carrytalk_macos_sc_last_error(const carrytalk_macos_sc_stream_handle *handle) {
    if (handle == NULL || handle->last_error[0] == '\0') {
        return NULL;
    }
    return handle->last_error;
}
