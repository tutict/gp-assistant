package com.gpassistant.credentials

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.security.KeyStore
import java.util.UUID

/** Instrumentation only: isolated random alias/directory; never accesses a user's credentials. */
@RunWith(AndroidJUnit4::class)
class CredentialVaultTest {
    private lateinit var vault: CredentialVault
    private lateinit var alias: String
    private val reference = "gp-assistant.llm.instrumentation"
    private val secret = "synthetic-instrumentation-key"
    @Before fun setUp() {
        alias = "gp.credentials.test." + UUID.randomUUID().toString()
        vault = CredentialVault(InstrumentationRegistry.getInstrumentation().targetContext, alias, alias)
    }
    @After fun tearDown() {
        vault.delete(reference)
        KeyStore.getInstance("AndroidKeyStore").apply { load(null); deleteEntry(alias) }
    }
    @Test fun testRoundTripWithoutPlaintextAndDelete() {
        vault.write(reference, secret)
        assertEquals(secret, vault.read(reference))
        assertFalse(vault.file(reference).readBytes().toString(Charsets.UTF_8).contains(secret))
        vault.delete(reference)
        assertNull(vault.read(reference))
    }
    @Test fun testTamperingFailsClosed() {
        vault.write(reference, secret)
        val bytes = vault.file(reference).readBytes()
        bytes[bytes.lastIndex] = (bytes.last().toInt() xor 1).toByte()
        vault.file(reference).writeBytes(bytes)
        try { vault.read(reference); fail("Tampering must not decrypt") } catch (_: java.security.GeneralSecurityException) { }
    }
    @Test fun testMissingKeyDoesNotRegenerateOnRead() {
        vault.write(reference, secret)
        KeyStore.getInstance("AndroidKeyStore").apply { load(null); deleteEntry(alias) }
        try { vault.read(reference); fail("Missing key must fail") } catch (_: IllegalStateException) { }
    }
}
