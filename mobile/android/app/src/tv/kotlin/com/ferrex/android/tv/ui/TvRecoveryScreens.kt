package com.ferrex.android.tv.ui

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.auth.ConnectResult
import com.ferrex.android.core.auth.LoginRequiredReason
import com.ferrex.android.core.auth.LoginResult
import com.ferrex.android.core.auth.NoServerReason
import com.ferrex.android.core.auth.RecoverableFailureReason
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.core.tvfocus.TvAuthFocusTarget
import com.ferrex.android.core.tvfocus.TvAuthRecoveryPolicy
import com.ferrex.android.tv.ui.foundation.TvActionPanel
import com.ferrex.android.tv.ui.foundation.TvActionPanelAction
import com.ferrex.android.tv.ui.foundation.TvActionRole
import com.ferrex.android.tv.ui.foundation.TvFocusableButton
import com.ferrex.android.tv.ui.foundation.TvFocusableStyle
import com.ferrex.android.tv.ui.foundation.TvFocusRestorer
import com.ferrex.android.tv.ui.foundation.TvScaffold
import com.ferrex.android.tv.ui.foundation.TvTitle
import com.ferrex.android.tv.ui.foundation.rememberTvFocusRestorer
import kotlinx.coroutines.launch

@Composable
fun TvLoadingScreen() {
    TvScaffold {
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
    val actionFocus = rememberTvFocusRestorer("server")
    var serverUrl by remember(state.previousServerUrl) { mutableStateOf(state.previousServerUrl.orEmpty()) }
    var message by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var serverFocusTarget by remember(state.reason, state.previousServerUrl) {
        mutableStateOf<TvAuthFocusTarget?>(TvAuthRecoveryPolicy.initialServerFocusTarget())
    }

    LaunchedEffect(busy, serverFocusTarget, state.reason, state.previousServerUrl) {
        if (!busy && serverFocusTarget == TvAuthFocusTarget.ServerUrl) {
            focusRequester.safeRequestFocus()
        }
    }

    fun connect() {
        if (busy) return
        scope.launch {
            busy = true
            message = null
            val result = onConnect(serverUrl)
            when (result) {
                is ConnectResult.Success -> message = if (result.setupStatus.canUsePasswordLogin) {
                    "Server reached. Sign in next."
                } else {
                    "Server reached, but setup is not complete."
                }
                is ConnectResult.Error -> message = result.message
            }
            serverFocusTarget = TvAuthRecoveryPolicy.afterServerConnectResult(
                succeeded = result is ConnectResult.Success,
            )
            busy = false
        }
    }

    TvScaffold {
        TvTitle("Connect to Ferrex", serverSubcopy(state.reason))
        state.previousServerUrl?.let { Text("Current server: $it", style = MaterialTheme.typography.bodyLarge) }
        Spacer(Modifier.height(28.dp))
        OutlinedTextField(
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(focusRequester)
                .tvSubmitOnEnter(
                    enabled = !busy,
                    includeDpadCenter = serverUrl.isNotBlank(),
                    onSubmit = { connect() },
                ),
            value = serverUrl,
            onValueChange = { serverUrl = it },
            label = { Text("Server URL, such as http://192.168.1.100:3000") },
            singleLine = true,
            enabled = !busy,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { connect() }),
        )
        Spacer(Modifier.height(18.dp))
        TvActionPanel(
            actions = listOf(
                TvActionPanelAction(
                    key = "connect",
                    label = "Retry / Connect",
                    role = TvActionRole.Retry,
                    enabled = !busy && serverUrl.isNotBlank(),
                    busy = busy,
                    onSelect = { connect() },
                ),
                TvActionPanelAction(
                    key = "reset-connection",
                    label = "Reset connection",
                    role = TvActionRole.SettingsExit,
                    onSelect = onResetConnection,
                ),
            ),
            focusRestorer = actionFocus,
            surfaceKey = "server-actions",
            autoFocus = false,
            buttonMaxWidth = 920.dp,
        )
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
    onOpenDiagnostics: () -> Unit,
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
    var loginFocusTarget by remember(state.reason, isFatal) {
        mutableStateOf<TvAuthFocusTarget?>(TvAuthRecoveryPolicy.initialLoginFocusTarget(isFatal))
    }

    fun requestLoginFocus(target: TvAuthFocusTarget?) {
        when (target) {
            TvAuthFocusTarget.Username -> firstFocus.safeRequestFocus()
            TvAuthFocusTarget.Password -> passwordFocus.safeRequestFocus()
            TvAuthFocusTarget.ServerUrl,
            TvAuthFocusTarget.RecoveryActions,
            null -> Unit
        }
    }

    LaunchedEffect(busy, loginFocusTarget, state.reason, isFatal) {
        if (!busy) {
            requestLoginFocus(loginFocusTarget)
        }
    }

    fun login() {
        if (busy) return
        scope.launch {
            busy = true
            message = null
            val result = onLogin(username, password)
            when (result) {
                is LoginResult.Success -> message = if (result.requiresPinSetup) {
                    "Signed in. PIN setup is required before PIN sign-in is available."
                } else {
                    "Signed in."
                }
                is LoginResult.Error -> message = result.message
            }
            loginFocusTarget = TvAuthRecoveryPolicy.afterLoginResult(
                succeeded = result is LoginResult.Success,
                username = username,
                password = password,
            )
            busy = false
        }
    }

    TvScaffold {
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
                    .focusRequester(firstFocus)
                    .tvSubmitOnEnter(
                        enabled = !busy,
                        includeDpadCenter = username.isNotBlank(),
                        onSubmit = { passwordFocus.safeRequestFocus() },
                    ),
                value = username,
                onValueChange = { username = it },
                label = { Text("Username") },
                singleLine = true,
                enabled = !busy,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                keyboardActions = KeyboardActions(onNext = { passwordFocus.safeRequestFocus() }),
            )
            Spacer(Modifier.height(16.dp))
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(passwordFocus)
                    .tvSubmitOnEnter(
                        enabled = !busy,
                        includeDpadCenter = username.isNotBlank() && password.isNotBlank(),
                        onSubmit = { login() },
                    ),
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
            TvFocusableButton(
                label = "Sign in",
                enabled = !busy && username.isNotBlank() && password.isNotBlank(),
                style = TvFocusableStyle.Primary,
                onClick = { login() },
                modifier = Modifier.fillMaxWidth(),
            ) {
                if (busy) {
                    CircularProgressIndicator(
                        modifier = Modifier
                            .padding(end = 8.dp)
                            .size(22.dp),
                        strokeWidth = 2.dp,
                    )
                }
                Text("Sign in", style = MaterialTheme.typography.titleMedium)
            }
        }
        TvRecoveryActions(
            screenKey = "login",
            includeRetry = true,
            autoFocus = isFatal,
            onRetry = onRetry,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onOpenDiagnostics = onOpenDiagnostics,
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
    onOpenDiagnostics: () -> Unit,
) {
    TvScaffold {
        TvTitle(recoverableTitle(state.reason), "Current server: ${state.serverUrl}")
        Text(recoverableCopy(state.reason), style = MaterialTheme.typography.titleLarge)
        TvRecoveryActions(
            screenKey = "recovery",
            includeRetry = true,
            autoFocus = true,
            onRetry = onRetry,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
            onOpenDiagnostics = onOpenDiagnostics,
        )
    }
}

@Composable
private fun TvRecoveryActions(
    screenKey: String,
    includeRetry: Boolean = true,
    autoFocus: Boolean,
    focusRestorer: TvFocusRestorer? = null,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
    onOpenDiagnostics: () -> Unit,
) {
    val restorer = focusRestorer ?: rememberTvFocusRestorer(screenKey)
    Spacer(Modifier.height(32.dp))
    TvActionPanel(
        title = "Recovery actions",
        actions = buildList {
            if (includeRetry) {
                add(
                    TvActionPanelAction(
                        key = "retry",
                        label = "Retry",
                        role = TvActionRole.Retry,
                        onSelect = onRetry,
                    ),
                )
            }
            add(
                TvActionPanelAction(
                    key = "sign-out",
                    label = "Sign out",
                    role = TvActionRole.Recovery,
                    onSelect = onSignOut,
                ),
            )
            add(
                TvActionPanelAction(
                    key = "change-server",
                    label = "Change server",
                    role = TvActionRole.SettingsExit,
                    onSelect = onChangeServer,
                ),
            )
            add(
                TvActionPanelAction(
                    key = "reset-connection",
                    label = "Reset connection",
                    role = TvActionRole.Destructive,
                    onSelect = onResetConnection,
                ),
            )
            add(
                TvActionPanelAction(
                    key = "diagnostics",
                    label = "Diagnostics / Export diagnostics",
                    role = TvActionRole.SettingsExit,
                    onSelect = onOpenDiagnostics,
                ),
            )
        },
        focusRestorer = restorer,
        surfaceKey = "recovery-actions",
        autoFocus = autoFocus,
    )
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

private fun FocusRequester.safeRequestFocus() {
    runCatching { requestFocus() }
}

private fun Modifier.tvSubmitOnEnter(
    enabled: Boolean,
    includeDpadCenter: Boolean = false,
    onSubmit: () -> Unit,
): Modifier = if (!enabled) {
    this
} else {
    onPreviewKeyEvent { event ->
        if (!event.key.isTvSubmitKey(includeDpadCenter)) return@onPreviewKeyEvent false
        when (event.type) {
            KeyEventType.KeyDown -> true
            KeyEventType.KeyUp -> {
                onSubmit()
                true
            }
            else -> false
        }
    }
}

private fun Key.isTvSubmitKey(includeDpadCenter: Boolean): Boolean =
    this == Key.Enter ||
        this == Key.NumPadEnter ||
        (includeDpadCenter && this == Key.DirectionCenter)

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
