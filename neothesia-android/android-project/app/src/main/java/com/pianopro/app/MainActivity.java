package com.pianopro.app;

import android.app.Activity;
import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.provider.OpenableColumns;
import android.view.WindowManager;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;

public class MainActivity extends NativeActivity {

    private static final int MIDI_PICK_CODE = 0x4D49;

    static {
        System.loadLibrary("neothesia_android");
    }

    @Override
    protected void onResume() {
        super.onResume();
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
    }

    @Override
    protected void onPause() {
        super.onPause();
        getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
    }

    public void startFilePicker() {
        Intent intent = new Intent(Intent.ACTION_GET_CONTENT);
        intent.setType("*/*");
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        startActivityForResult(Intent.createChooser(intent, "Select MIDI file"), MIDI_PICK_CODE);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != MIDI_PICK_CODE) return;

        if (resultCode != Activity.RESULT_OK || data == null || data.getData() == null) {
            onMidiPickResult(null, null);
            return;
        }

        Uri uri = data.getData();
        String name = getDisplayName(uri);
        if (name == null) {
            String path = uri.getLastPathSegment();
            name = path != null ? path : "song.mid";
        }

        try {
            InputStream stream = getContentResolver().openInputStream(uri);
            if (stream == null) {
                onMidiPickResult(null, null);
                return;
            }
            byte[] bytes = readAllBytes(stream);
            stream.close();
            onMidiPickResult(bytes, name);
        } catch (Exception e) {
            onMidiPickResult(null, null);
        }
    }

    private String getDisplayName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(
                uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                return cursor.getString(0);
            }
        } catch (Exception ignored) {}
        return null;
    }

    private static byte[] readAllBytes(InputStream stream) throws Exception {
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        byte[] tmp = new byte[8192];
        int n;
        while ((n = stream.read(tmp)) != -1) {
            buf.write(tmp, 0, n);
        }
        return buf.toByteArray();
    }

    private native void onMidiPickResult(byte[] bytes, String name);
}
