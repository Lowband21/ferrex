package com.ferrex.android.tv.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.ferrex.android.FerrexShellCopy
import com.ferrex.android.core.auth.ConnectResult
import com.ferrex.android.core.auth.LoginRequiredReason
import com.ferrex.android.core.auth.LoginResult
import com.ferrex.android.core.auth.NoServerReason
import com.ferrex.android.core.auth.RecoverableFailureReason
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.browse.LibraryBrowseModels
import com.ferrex.android.core.detail.DetailCache
import com.ferrex.android.core.detail.DetailLoadResult
import com.ferrex.android.core.detail.DetailRouteContracts
import com.ferrex.android.core.image.FerrexImagePipeline
import com.ferrex.android.core.image.ImageRepository
import com.ferrex.android.core.library.LibraryFreshness
import com.ferrex.android.core.library.LibraryRepository
import com.ferrex.android.core.library.LibraryRepositoryState
import com.ferrex.android.core.library.ServerCacheScope
import com.ferrex.android.core.playback.PlaybackProgressReporter
import com.ferrex.android.core.playback.PlaybackRouteContract
import com.ferrex.android.core.playback.PlaybackStreamUrlFactory
import com.ferrex.android.core.playback.PlaybackTicketTransport
import com.ferrex.android.ui.components.FerrexBrowseImageRail
import com.ferrex.android.ui.player.PlayerChrome
import com.ferrex.android.ui.player.PlayerScreen
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient

@Composable
fun TvLoadingScreen() {
    TvSurface {
        CircularProgressIndicator()
        Spacer(Modifier.height(24.dp))
        Text(
            text = "Checking Ferrex session…",
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
fun TvServerConnectScreen(
    state: SessionState.NoServer,
    onConnect: suspend (String) -> ConnectResult,
    onResetConnection: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val focusRequester = remember { FocusRequester() }
    var serverUrl by remember(state.previousServerUrl) { mutableStateOf(state.previousServerUrl.orEmpty()) }
    var message by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    LaunchedEffect(state.reason) {
        runCatching { focusRequester.requestFocus() }
    }

    fun connect() {
        scope.launch {
            busy = true
            message = null
            when (val result = onConnect(serverUrl)) {
                is ConnectResult.Success -> message = if (result.setupStatus.canUsePasswordLogin) {
                    "Server reached. Sign in next."
                } else {
                    "Server reached, but setup is not complete."
                }
                is ConnectResult.Error -> message = result.message
            }
            busy = false
        }
    }

    TvSurface {
        TvTitle("Connect to Ferrex", serverSubcopy(state.reason))
        state.previousServerUrl?.let { Text("Current server: $it", style = MaterialTheme.typography.bodyLarge) }
        Spacer(Modifier.height(28.dp))
        OutlinedTextField(
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(focusRequester),
            value = serverUrl,
            onValueChange = { serverUrl = it },
            label = { Text("Server URL, such as http://192.168.1.100:3000") },
            singleLine = true,
            enabled = !busy,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { connect() }),
        )
        Spacer(Modifier.height(18.dp))
        Button(
            modifier = Modifier.fillMaxWidth(),
            enabled = !busy && serverUrl.isNotBlank(),
            onClick = { connect() },
        ) {
            if (busy) CircularProgressIndicator(modifier = Modifier.padding(end = 12.dp))
            Text("Retry / Connect")
        }
        TextButton(onClick = onResetConnection, modifier = Modifier.fillMaxWidth()) {
            Text("Reset connection")
        }
        TvMessage(message)
    }
}

@Composable
fun TvLoginScreen(
    state: SessionState.NeedsLogin,
    onLogin: suspend (String, String) -> LoginResult,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    val isFatal = state.reason == LoginRequiredReason.SetupRequired ||
        state.reason == LoginRequiredReason.RegistrationClosed
    val firstFocus = remember { FocusRequester() }
    val passwordFocus = remember { FocusRequester() }
    val scope = rememberCoroutineScope()
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var message by remember(state.reason) { mutableStateOf<String?>(loginReasonCopy(state.reason)) }
    var busy by remember { mutableStateOf(false) }

    LaunchedEffect(state.reason) {
        runCatching { firstFocus.requestFocus() }
    }

    fun login() {
        scope.launch {
            busy = true
            message = null
            when (val result = onLogin(username, password)) {
                is LoginResult.Success -> message = if (result.requiresPinSetup) {
                    "Signed in. PIN setup is required before PIN sign-in is available."
                } else {
                    "Signed in."
                }
                is LoginResult.Error -> message = result.message
            }
            busy = false
        }
    }

    TvSurface {
        TvTitle(if (isFatal) "Server action required" else "Sign in", "Current server: ${state.serverUrl}")
        Text(
            text = if (isFatal) setupCopy(state.reason) else "Use Ferrex device password sign-in. PIN requirements are reported without showing fake setup routes.",
            style = MaterialTheme.typography.titleLarge,
        )
        Spacer(Modifier.height(28.dp))
        if (!isFatal) {
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(firstFocus),
                value = username,
                onValueChange = { username = it },
                label = { Text("Username") },
                singleLine = true,
                enabled = !busy,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                keyboardActions = KeyboardActions(onNext = { passwordFocus.requestFocus() }),
            )
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(passwordFocus),
                value = password,
                onValueChange = { password = it },
                label = { Text("Password") },
                singleLine = true,
                enabled = !busy,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { login() }),
            )
            Spacer(Modifier.height(18.dp))
            Button(
                modifier = Modifier.fillMaxWidth(),
                enabled = !busy && username.isNotBlank() && password.isNotBlank(),
                onClick = { login() },
            ) {
                if (busy) CircularProgressIndicator(modifier = Modifier.padding(end = 12.dp))
                Text("Sign in")
            }
        }
        TvRecoveryActions(
            firstFocusRequester = if (isFatal) firstFocus else null,
            onRetry = onRetry,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
        )
        TvMessage(message)
    }
}

@Composable
fun TvRecoverableScreen(
    state: SessionState.RecoverableFailure,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    val firstFocus = remember { FocusRequester() }
    LaunchedEffect(state.reason) {
        runCatching { firstFocus.requestFocus() }
    }

    TvSurface {
        TvTitle(recoverableTitle(state.reason), "Current server: ${state.serverUrl}")
        Text(recoverableCopy(state.reason), style = MaterialTheme.typography.titleLarge)
        TvRecoveryActions(
            firstFocusRequester = firstFocus,
            onRetry = onRetry,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
        )
    }
}

@Composable
fun TvHomeScreen(
    state: SessionState.Authenticated,
    libraryRepository: LibraryRepository? = null,
    imageRepository: ImageRepository? = null,
    imagePipeline: FerrexImagePipeline? = null,
    playbackTicketTransport: PlaybackTicketTransport? = null,
    playbackStreamUrlFactory: PlaybackStreamUrlFactory? = null,
    playbackProgressReporter: PlaybackProgressReporter? = null,
    streamingHttpClient: OkHttpClient? = null,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onPlaybackSessionInvalidated: () -> Unit = {},
) {
    val scope = remember(state.serverUrl, state.user.id) { ServerCacheScope.from(state.serverUrl, state.user.id) }
    val emptyRepositoryState = remember { mutableStateOf<LibraryRepositoryState?>(null) }
    val repositoryState by libraryRepository?.state?.collectAsState() ?: emptyRepositoryState
    val coroutineScope = rememberCoroutineScope()
    var activePlaybackContract by remember { mutableStateOf<PlaybackRouteContract?>(null) }
    var playbackNotice by remember { mutableStateOf<String?>(null) }
    val firstPlaybackContract = remember(repositoryState) { firstTvPlaybackContract(repositoryState) }
    val playbackReady = playbackTicketTransport != null && playbackStreamUrlFactory != null && streamingHttpClient != null

    LaunchedEffect(libraryRepository, scope) {
        libraryRepository?.refreshLibraries(scope)
    }

    val playbackContract = activePlaybackContract
    if (playbackContract != null && playbackTicketTransport != null && playbackStreamUrlFactory != null && streamingHttpClient != null) {
        PlayerScreen(
            route = playbackContract,
            ticketTransport = playbackTicketTransport,
            streamUrlFactory = playbackStreamUrlFactory,
            progressReporter = playbackProgressReporter,
            resumeProgressProvider = null,
            streamingHttpClient = streamingHttpClient,
            chrome = PlayerChrome.Tv,
            onBack = { activePlaybackContract = null },
            onSessionInvalidated = {
                activePlaybackContract = null
                onPlaybackSessionInvalidated()
            },
            onProgressCommitted = {},
            onChangeServer = onChangeServer,
            onSignOut = onSignOut,
        )
        return
    }

    TvSurface {
        TvTitle(FerrexShellCopy.TV_TITLE, FerrexShellCopy.TV_SUBTITLE)
        Text("Signed in as ${state.user.displayName ?: state.user.username}", style = MaterialTheme.typography.headlineSmall)
        Text("Server: ${state.serverUrl}", style = MaterialTheme.typography.titleMedium)
        Text(FerrexShellCopy.TV_BODY, style = MaterialTheme.typography.titleLarge)
        if (state.requiresPinSetup) {
            Text(
                text = "PIN setup is required by this server before PIN sign-in can be used. Use password sign-in or configure PIN support on the server.",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.primary,
            )
        }
        TvLibraryCacheStatus(
            freshness = repositoryState?.freshness ?: LibraryFreshness.Empty,
            selectedLibraryName = repositoryState?.libraries?.firstOrNull { it.id == repositoryState?.selectedLibraryId }?.name,
            selectedLibraryId = repositoryState?.selectedLibraryId,
            movieCount = repositoryState?.movieAccessor?.movieCount,
            seriesBundleCount = repositoryState?.seriesAccessor?.bundleCount,
            onRetry = { coroutineScope.launch { libraryRepository?.refreshLibraries(scope, repositoryState?.selectedLibraryId) } },
            onClearSelected = {
                val libraryId = repositoryState?.selectedLibraryId ?: return@TvLibraryCacheStatus
                libraryRepository?.clearSelectedCache(scope, libraryId)
            },
            onClearAll = { libraryRepository?.clearAllCache(scope) },
        )
        TvPlaybackEntry(
            contract = firstPlaybackContract,
            playbackNotice = playbackNotice,
            onLaunch = { contract ->
                if (!playbackReady) {
                    playbackNotice = "Playback is unavailable because the ticketed Media3 substrate is not configured."
                } else {
                    playbackNotice = null
                    activePlaybackContract = contract
                }
            },
        )
        FerrexBrowseImageRail(
            modifier = Modifier.padding(top = 28.dp),
            repositoryState = repositoryState,
            scope = scope,
            imageRepository = imageRepository,
            imagePipeline = imagePipeline,
            maxImages = 10,
            itemWidth = 180.dp,
            horizontalAlignment = Alignment.CenterHorizontally,
        )
        TvRecoveryActions(
            includeRetry = false,
            firstFocusRequester = null,
            onRetry = {},
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
        )
    }
}

@Composable
private fun TvPlaybackEntry(
    contract: PlaybackRouteContract?,
    playbackNotice: String?,
    onLaunch: (PlaybackRouteContract) -> Unit,
) {
    Spacer(Modifier.height(18.dp))
    val copy = when {
        contract != null -> "Open the first cached playable item with TV D-pad controls and audio/subtitle pickers."
        else -> "TV playback controls will become available after a cached playable movie or episode is present."
    }
    Text(copy, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    Button(
        enabled = contract != null,
        onClick = { contract?.let(onLaunch) },
        modifier = Modifier.width(360.dp),
    ) {
        Text("Play first cached item")
    }
    playbackNotice?.let {
        Text(it, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.primary, textAlign = TextAlign.Center)
    }
}

private fun firstTvPlaybackContract(state: LibraryRepositoryState?): PlaybackRouteContract? {
    state ?: return null

    state.movieLibraries
        .asSequence()
        .flatMap { LibraryBrowseModels.movieGridCards(it).asSequence() }
        .mapNotNull { card -> DetailCache.resolve(state, card.route) as? DetailLoadResult.Movie }
        .mapNotNull { detail -> DetailRouteContracts.movieStartOver(detail.detail, detail.route) }
        .firstOrNull()
        ?.let { return it }

    return state.seriesLibraries
        .asSequence()
        .flatMap { LibraryBrowseModels.seriesGridCards(it).asSequence() }
        .mapNotNull { card -> DetailCache.resolve(state, card.route) as? DetailLoadResult.Series }
        .mapNotNull { detail -> DetailRouteContracts.seriesStartOver(detail.detail, detail.route) }
        .firstOrNull()
}

@Composable
private fun TvLibraryCacheStatus(
    freshness: LibraryFreshness,
    selectedLibraryName: String?,
    selectedLibraryId: String?,
    movieCount: Int?,
    seriesBundleCount: Int?,
    onRetry: () -> Unit,
    onClearSelected: () -> Unit,
    onClearAll: () -> Unit,
) {
    Text(
        text = "Library cache: ${freshness.label}",
        style = MaterialTheme.typography.titleLarge,
        color = MaterialTheme.colorScheme.primary,
        textAlign = TextAlign.Center,
    )
    selectedLibraryName?.let { Text("Selected library: $it", style = MaterialTheme.typography.titleMedium) }
    val countCopy = when {
        movieCount != null -> "Movies cached across all batches: $movieCount"
        seriesBundleCount != null -> "Series bundles cached: $seriesBundleCount"
        else -> "No cached library payloads yet."
    }
    Text(countCopy, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    val detail = when (freshness) {
        LibraryFreshness.Empty -> "Cache will build after a reachable library sync."
        is LibraryFreshness.Fresh -> "Fresh for this server and user."
        LibraryFreshness.Syncing -> "Syncing library payloads…"
        is LibraryFreshness.StaleOffline -> "Showing stale/offline cache: ${freshness.message}"
        is LibraryFreshness.CorruptRebuilding -> freshness.message
        is LibraryFreshness.ErrorRetryable -> "Retryable cache sync issue: ${freshness.message}"
    }
    Text(detail, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
    Spacer(Modifier.height(18.dp))
    Button(onClick = onRetry, modifier = Modifier.width(360.dp)) { Text("Retry cache sync") }
    TextButton(
        enabled = selectedLibraryId != null,
        onClick = onClearSelected,
        modifier = Modifier.width(360.dp),
    ) { Text("Clear selected cache") }
    TextButton(onClick = onClearAll, modifier = Modifier.width(360.dp)) { Text("Clear all server cache") }
}

@Composable
private fun TvSurface(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF070A12)),
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        listOf(Color(0xFF172554), Color(0xFF070A12), Color(0xFF020617)),
                    ),
                )
                .padding(horizontal = 112.dp, vertical = 72.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
            content = content,
        )
    }
}

@Composable
private fun TvTitle(title: String, subtitle: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.displaySmall,
        color = MaterialTheme.colorScheme.primary,
        fontWeight = FontWeight.Bold,
        textAlign = TextAlign.Center,
    )
    Spacer(Modifier.height(12.dp))
    Text(
        text = subtitle,
        style = MaterialTheme.typography.titleLarge,
        textAlign = TextAlign.Center,
    )
    Spacer(Modifier.height(28.dp))
}

@Composable
private fun TvRecoveryActions(
    includeRetry: Boolean = true,
    firstFocusRequester: FocusRequester?,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    Spacer(Modifier.height(32.dp))
    if (includeRetry) {
        Button(
            modifier = Modifier
                .width(360.dp)
                .then(if (firstFocusRequester != null) Modifier.focusRequester(firstFocusRequester) else Modifier),
            onClick = onRetry,
        ) {
            Text("Retry")
        }
        Spacer(Modifier.height(14.dp))
    }
    Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
        TextButton(onClick = onSignOut, modifier = Modifier.width(220.dp)) { Text("Sign out") }
        TextButton(onClick = onChangeServer, modifier = Modifier.width(220.dp)) { Text("Change server") }
    }
    TextButton(onClick = onResetConnection, modifier = Modifier.width(360.dp)) {
        Text("Reset connection")
    }
}

@Composable
private fun TvMessage(message: String?) {
    message?.let {
        Spacer(Modifier.height(20.dp))
        Text(
            text = it,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            textAlign = TextAlign.Center,
        )
    }
}

private fun serverSubcopy(reason: NoServerReason): String = when (reason) {
    NoServerReason.FirstInstall -> "Enter the server address for this TV."
    NoServerReason.ResetConnection -> "Connection data was reset without requiring an OS app-data wipe."
    NoServerReason.ChangeServer -> "Choose a different server. The saved URL changes only after a successful check."
}

private fun loginReasonCopy(reason: LoginRequiredReason): String? = when (reason) {
    LoginRequiredReason.NoSavedSession -> null
    LoginRequiredReason.SignedOut -> "Signed out locally."
    LoginRequiredReason.SessionExpired -> "Session expired. Sign in again or use a recovery action."
    LoginRequiredReason.SessionRevoked -> "Session was revoked or could not be refreshed."
    LoginRequiredReason.RefreshFailed -> "Refresh failed because saved tokens were not usable."
    LoginRequiredReason.SetupRequired -> "This server still needs first-run setup."
    LoginRequiredReason.RegistrationClosed -> "This server is not accepting Android account setup."
    LoginRequiredReason.ChangedServer -> "Server changed. Sign in to continue."
}

private fun setupCopy(reason: LoginRequiredReason): String = when (reason) {
    LoginRequiredReason.SetupRequired -> "Complete setup through the supported server path, then retry. This TV app does not create an admin account."
    LoginRequiredReason.RegistrationClosed -> "An administrator must create or enable an account. This TV app does not show a fake registration route."
    else -> "Sign in is unavailable until the server is ready."
}

private fun recoverableTitle(reason: RecoverableFailureReason): String = when (reason) {
    RecoverableFailureReason.ServerUnreachable -> "Server unreachable"
    RecoverableFailureReason.ValidationUnavailable -> "Validation unavailable"
    RecoverableFailureReason.RefreshUnavailable -> "Refresh unavailable"
    RecoverableFailureReason.InvalidServerResponse -> "Server response changed"
}

private fun recoverableCopy(reason: RecoverableFailureReason): String = when (reason) {
    RecoverableFailureReason.ServerUnreachable -> "Saved tokens and server URL were preserved. Protected TV screens are closed until the session validates."
    RecoverableFailureReason.ValidationUnavailable -> "Ferrex could not validate the restored session. Retry, sign out, change server, or reset connection."
    RecoverableFailureReason.RefreshUnavailable -> "Ferrex could not refresh the session right now. Recovery actions are D-pad reachable."
    RecoverableFailureReason.InvalidServerResponse -> "Ferrex reached the server but could not understand the auth response. Retry after updating or recover locally."
}
