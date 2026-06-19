package com.ferrex.android.ui.qa

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FerrexVisualQaScenariosTest {
    @Test
    fun registryIncludesEveryRequiredScenarioIdInStableOrder() {
        assertEquals(FerrexVisualQaScenarios.requiredScenarioIds, FerrexVisualQaScenarios.all.map { it.id })
        assertTrue(FerrexVisualQaScenarios.all.any { it.device == VisualQaDevice.Phone })
        assertTrue(FerrexVisualQaScenarios.all.any { it.device == VisualQaDevice.Tv })
    }

    @Test
    fun debugLaunchGateAcceptsScenariosAndReleaseRejectsThem() {
        assertTrue(FerrexVisualQaLaunch.isEnabled(isDebugBuild = true))
        assertFalse(FerrexVisualQaLaunch.isEnabled(isDebugBuild = false))

        assertEquals(
            FerrexQaScenarioIds.PhoneSearch,
            FerrexVisualQaLaunch.resolveScenarioId(FerrexQaScenarioIds.PhoneSearch, isDebugBuild = true),
        )
        assertEquals(
            FerrexVisualQaScenarios.defaultScenario.id,
            FerrexVisualQaLaunch.resolveScenarioId("missing-scenario", isDebugBuild = true),
        )
        assertEquals(
            FerrexVisualQaScenarios.defaultScenario.id,
            FerrexVisualQaLaunch.resolveScenarioId(null, isDebugBuild = true),
        )
        assertEquals(null, FerrexVisualQaLaunch.resolveScenarioId(FerrexQaScenarioIds.PhoneSearch, isDebugBuild = false))
    }

    @Test
    fun theaterPlateScenariosCoverRequiredStatesForPhoneAndTv() {
        val theaterScenarios = FerrexVisualQaScenarios.all.filter { it.kind == VisualQaScenarioKind.TheaterPlate }
        val expectedStates = VisualQaTheaterPlateState.entries.toSet()
        val requiredStateKeys = listOf(
            "bright",
            "dark",
            "busy",
            "missing-backdrop",
            "long-title",
            "missing-artwork",
            "stale-offline",
            "recovery",
            "search",
            "browse",
            "detail",
            "rails",
            "playback-entry",
        )

        assertEquals(expectedStates.size * 2, theaterScenarios.size)
        assertEquals(expectedStates, theaterScenarios.filter { it.device == VisualQaDevice.Phone }.mapNotNull { it.theaterPlateState }.toSet())
        assertEquals(expectedStates, theaterScenarios.filter { it.device == VisualQaDevice.Tv }.mapNotNull { it.theaterPlateState }.toSet())
        assertEquals(requiredStateKeys, VisualQaTheaterPlateState.entries.map { it.key })
    }

    @Test
    fun tvTheaterPlateScenariosExposeStableRequiredTags() {
        val tvScenarios = FerrexVisualQaScenarios.all.filter {
            it.device == VisualQaDevice.Tv && it.kind == VisualQaScenarioKind.TheaterPlate
        }
        val requiredStateKeys = VisualQaTheaterPlateState.entries.map { it.key }

        assertEquals(requiredStateKeys.map { "tv-theater-plate-$it" }, tvScenarios.map { it.id })
        tvScenarios.forEach { scenario ->
            val state = requireNotNull(scenario.theaterPlateState)
            assertEquals(FerrexQaTags.TheaterPlate.root("tv", state.key), scenario.testTag)
            assertEquals("Debug Visual QA → TV Theater Plate → ${state.label}", scenario.evidencePath)
            assertTrue("${scenario.id} fixture includes state key", scenario.fixtureSamples.contains(state.key))
        }
    }

    @Test
    fun scenarioIdsTagsAndFixtureSamplesAreUniqueAndStable() {
        val scenarios = FerrexVisualQaScenarios.all

        assertEquals(scenarios.size, scenarios.map { it.id }.distinct().size)
        assertEquals(scenarios.size, scenarios.map { it.testTag }.distinct().size)
        scenarios.forEach { scenario ->
            assertTrue("${scenario.id} id", scenario.id.matches(Regex("[a-z0-9-]+")))
            assertTrue("${scenario.id} tag", scenario.testTag.matches(Regex("[a-z0-9_.-]+")))
            assertTrue("${scenario.id} title", scenario.title.isNotBlank())
            assertTrue("${scenario.id} description", scenario.description.length >= 24)
            assertTrue("${scenario.id} evidence", scenario.evidencePath.isNotBlank())
            assertEquals(
                "${scenario.id} fixture samples must be unique",
                scenario.fixtureSamples.size,
                scenario.fixtureSamples.distinct().size,
            )
            scenario.fixtureSamples.forEach { sample -> assertTrue("${scenario.id} sample", sample.isNotBlank()) }
            assertNotNull(FerrexVisualQaScenarios.find(scenario.id))
        }
    }

    @Test
    fun fixturesAvoidPrivateServerAccountMediaAndArtworkValues() {
        val forbiddenPatterns = listOf(
            Regex("https?://", RegexOption.IGNORE_CASE),
            Regex("\\b(token|password|secret|bearer)\\b", RegexOption.IGNORE_CASE),
            Regex("(^|\\s)/(home|users|mnt|volumes|storage|sdcard)/", RegexOption.IGNORE_CASE),
            Regex("image\\.tmdb\\.org|/t/p/|\\.(jpg|jpeg|png|webp)\\b", RegexOption.IGNORE_CASE),
        )

        FerrexVisualQaFixtures.privacyScanStrings().forEach { sample ->
            forbiddenPatterns.forEach { pattern ->
                assertFalse("fixture value must stay synthetic: $sample", pattern.containsMatchIn(sample))
            }
        }
    }

    @Test
    fun noWipeRecoveryActionsStayVisibleInRecoveryScenarios() {
        val requiredLabels = setOf(
            "Retry",
            "Sign out",
            "Change server",
            "Reset connection",
            "Diagnostics / Export diagnostics",
        )
        val recoveryScenarios = FerrexVisualQaScenarios.all.filter { it.recoveryActions.isNotEmpty() }
        assertTrue(recoveryScenarios.isNotEmpty())

        recoveryScenarios.forEach { scenario ->
            val labels = scenario.recoveryActions.map { it.label }.toSet()
            assertTrue("${scenario.id} labels $labels", labels.containsAll(requiredLabels))
            if (scenario.kind == VisualQaScenarioKind.TheaterPlate) {
                assertTrue("${scenario.id} cache recovery labels $labels", labels.contains("Clear cache"))
            }
            scenario.recoveryActions.forEach { action ->
                assertTrue("${scenario.id} ${action.key} must stay enabled", action.enabled)
                assertFalse("${scenario.id} ${action.key} must not require app data wipes", action.requiresDataWipe)
                assertFalse(action.label.contains("pm clear", ignoreCase = true))
                assertFalse(action.label.contains("wipe", ignoreCase = true))
                assertFalse(action.label.contains("clear app data", ignoreCase = true))
            }
        }
    }
}
