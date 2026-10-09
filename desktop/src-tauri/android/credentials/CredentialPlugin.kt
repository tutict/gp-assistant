package com.gpassistant.credentials

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.system.Os
import android.system.OsConstants
import android.util.AtomicFile
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class CredentialArgs {
    lateinit var reference: String
    var secret: String? = null
}

/** Only Rust invokes this bridge; the Rust plugin rejects every JS plugin command. */
@TauriPlugin
class CredentialPlugin(private val activity: Activity) : Plugin(activity) {
    private val vault by lazy { CredentialVault(activity.applicationContext) }
    @Command fun write(invoke: Invoke) = safely(invoke) {
        val args = invoke.parseArgs(CredentialArgs::class.java)
        vault.write(args.reference, requireNotNull(args.secret))
        JSObject()
    }
    @Command fun read(invoke: Invoke) = safely(invoke) {
        val args = invoke.parseArgs(CredentialArgs::class.java)
        JSObject().apply { put("secret", vault.read(args.reference) ?: org.json.JSONObject.NULL) }
    }
    @Command fun delete(invoke: Invoke) = safely(invoke) {
        vault.delete(invoke.parseArgs(CredentialArgs::class.java).reference)
        JSObject()
    }
    private fun safely(invoke: Invoke, action: () -> JSObject) {
        try { invoke.resolve(action()) }
        catch (_: Exception) { invoke.reject("OS credential operation failed.") }
    }
}

/** Keystore AES key is non-exportable. Only authenticated ciphertext is ever written to disk. */
internal class CredentialVault(
    context: Context,
    private val alias: String = "com.gpassistant.llm.aes.v1",
    directory: String = "llm-credentials",
) {
    private val root = File(context.noBackupFilesDir, directory).apply {
        check(isDirectory || mkdirs()) { "Credential directory unavailable" }
    }
    private val keys get() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    internal fun file(reference: String): File {
        require(Regex("^gp-assistant\\.llm\\.[A-Za-z0-9_-]{1,96}$").matches(reference))
        return File(root, "$reference.enc")
    }
    private fun key(create: Boolean): SecretKey {
        val existing = keys.getKey(alias, null)
        if (existing != null) return existing as SecretKey
        check(create) { "Credential encryption key unavailable" }
        // No software/random-key fallback if AndroidKeyStore is unavailable.
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
            init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true).build())
        }.generateKey()
    }
    @Synchronized fun write(reference: String, secret: String) {
        val target = file(reference)
        val plaintext = secret.toByteArray(Charsets.UTF_8)
        try {
            require(plaintext.size in 1..2560 && secret.isNotBlank() && !secret.any { it == '\n' || it == '\r' || it == '\u0000' })
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key(true))
            cipher.updateAAD(reference.toByteArray(Charsets.UTF_8))
            check(cipher.iv.size == 12)
            val encrypted = byteArrayOf(1) + cipher.iv + cipher.doFinal(plaintext)
            val atomic = AtomicFile(target)
            val stream = atomic.startWrite()
            try { stream.write(encrypted); atomic.finishWrite(stream); syncDirectory() }
            catch (error: Exception) { atomic.failWrite(stream); throw error }
            check(read(reference) == secret) { "Credential verification failed" }
        } finally { plaintext.fill(0) }
    }
    @Synchronized fun read(reference: String): String? {
        val target = file(reference)
        val atomic = AtomicFile(target)
        // AtomicFile.openRead restores .bak after a crash. Missing differs from corrupt/inaccessible.
        if (!target.exists() && !File(target.path + ".bak").exists()) return null
        val bytes = atomic.openRead().use { input ->
            val buffer = ByteArray(4097)
            var count = 0
            while (count < buffer.size) {
                val n = input.read(buffer, count, buffer.size - count)
                if (n == -1) break
                count += n
            }
            require(count in 30..4096)
            buffer.copyOf(count)
        }
        require(bytes[0] == 1.toByte())
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(false), GCMParameterSpec(128, bytes.copyOfRange(1, 13)))
        cipher.updateAAD(reference.toByteArray(Charsets.UTF_8))
        val plaintext = cipher.doFinal(bytes.copyOfRange(13, bytes.size))
        return try { plaintext.toString(Charsets.UTF_8) } finally { plaintext.fill(0) }
    }
    @Synchronized fun delete(reference: String) {
        val target = file(reference)
        AtomicFile(target).delete()
        check(!target.exists() && !File(target.path + ".bak").exists() && !File(target.path + ".new").exists())
        syncDirectory()
    }
    private fun syncDirectory() {
        val fd = Os.open(root.path, OsConstants.O_RDONLY, 0)
        try { Os.fsync(fd) } finally { Os.close(fd) }
    }
}
