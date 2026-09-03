package dev.galileo.android

import org.junit.Assert.assertEquals
import org.junit.Test

class CallSiteTest {
    private fun loadHabits(): Map<String, Any?> = Galileo.callSite()

    @Test fun callSiteSkipsSdkFrames() {
        val cs = loadHabits()
        assertEquals("loadHabits", cs["code.function.name"])
        assertEquals("dev.galileo.android.CallSiteTest", cs["code.namespace"])
    }
}
