/* Exercise the staged GStreamer closure without linking to Homebrew dylibs. */

#include <gst/gst.h>
#include <gio/gio.h>
#include <dlfcn.h>
#include <stdio.h>

static int audit_factories(void) {
    static const char *required[] = {
        "playbin3",
        "appsink",
        "videoconvertscale",
        "scaletempo",
        "vtdec",
        "atdec",
        "qtdemux",
        "h264parse",
        "souphttpsrc",
        "osxaudiosink",
    };

    for (size_t index = 0; index < G_N_ELEMENTS(required); index++) {
        GstElementFactory *factory = gst_element_factory_find(required[index]);
        if (factory == NULL) {
            fprintf(stderr, "required clean-bundle factory is missing: %s\n", required[index]);
            return 1;
        }
        gst_object_unref(factory);
    }

    GstElementFactory *forbidden = gst_element_factory_find("avdec_h264");
    if (forbidden != NULL) {
        fprintf(stderr, "excluded gst-libav factory leaked into the bundle: avdec_h264\n");
        gst_object_unref(forbidden);
        return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    GstElement *pipeline;
    GstElement *audio_sink;
    GstElement *video_sink;
    GstBus *bus;
    GstMessage *message;
    GTlsDatabase *database;
    GError *tls_error = NULL;
    int result = 1;

    if (argc != 4) {
        fprintf(stderr, "usage: macos_gstreamer_bundle_smoke URL LIBSOUP CA_PEM\n");
        return 2;
    }
    if (dlopen(argv[2], RTLD_LAZY | RTLD_GLOBAL) == NULL) {
        fprintf(stderr, "could not preload bundled libsoup: %s\n", dlerror());
        return 1;
    }
    database = g_tls_file_database_new(argv[3], &tls_error);
    if (database == NULL) {
        fprintf(stderr, "could not load test CA database: %s\n", tls_error->message);
        g_clear_error(&tls_error);
        return 1;
    }
    g_tls_backend_set_default_database(g_tls_backend_get_default(), database);
    g_object_unref(database);
    gst_init(&argc, &argv);
    if (audit_factories() != 0) {
        return 1;
    }
    pipeline = gst_element_factory_make("playbin3", NULL);
    audio_sink = gst_element_factory_make("fakesink", NULL);
    video_sink = gst_element_factory_make("fakesink", NULL);
    if (pipeline == NULL || audio_sink == NULL || video_sink == NULL) {
        fprintf(stderr, "required clean-bundle GStreamer factories are missing\n");
        goto cleanup_elements;
    }
    g_object_set(audio_sink, "sync", FALSE, NULL);
    g_object_set(video_sink, "sync", FALSE, NULL);
    g_object_set(
        pipeline,
        "uri", argv[1],
        "audio-sink", audio_sink,
        "video-sink", video_sink,
        NULL
    );
    if (gst_element_set_state(pipeline, GST_STATE_PLAYING) == GST_STATE_CHANGE_FAILURE) {
        fprintf(stderr, "clean-bundle HLS pipeline could not start\n");
        goto cleanup_pipeline;
    }

    bus = gst_element_get_bus(pipeline);
    message = gst_bus_timed_pop_filtered(
        bus,
        30 * GST_SECOND,
        GST_MESSAGE_ERROR | GST_MESSAGE_EOS
    );
    if (message == NULL) {
        fprintf(stderr, "clean-bundle HLS pipeline timed out\n");
    } else if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_ERROR) {
        GError *error = NULL;
        gchar *debug = NULL;
        gst_message_parse_error(message, &error, &debug);
        fprintf(stderr, "clean-bundle HLS pipeline failed: %s\n", error->message);
        g_clear_error(&error);
        g_free(debug);
        gst_message_unref(message);
    } else {
        gst_message_unref(message);
        result = 0;
    }
    gst_object_unref(bus);

cleanup_pipeline:
    gst_element_set_state(pipeline, GST_STATE_NULL);
cleanup_elements:
    if (pipeline != NULL) gst_object_unref(pipeline);
    if (audio_sink != NULL) gst_object_unref(audio_sink);
    if (video_sink != NULL) gst_object_unref(video_sink);
    return result;
}
