package com.ferrex.android.ui.recovery

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
import kotlinx.coroutines.launch

@Composable
fun PhoneLoadingScreen() {
    PhoneSurface {
        CircularProgressIndicator()
        Text(
            modifier = Modifier.padding(top = 20.dp),
            text = "Checking your Ferrex connection…",
            style = MaterialTheme.typography.titleMedium,
        )
    }
}

@Composable
fun PhoneServerConnectScreen(
    state: SessionState.NoServer,
    onConnect: suspend (String) -> ConnectResult,
    onResetConnection: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var serverUrl by remember(state.previousServerUrl) { mutableStateOf(state.previousServerUrl.orEmpty()) }
    var message by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun connect() {
        scope.launch {
            busy = true
            message = null
            when (val result = onConnect(serverUrl)) {
                is ConnectResult.Success -> {
                    message = if (result.setupStatus.canUsePasswordLogin) {
                        "Server reached. Sign in with your Ferrex account."
                    } else {
                        "Server reached, but setup is not complete."
                    }
                }
                is ConnectResult.Error -> message = result.message
            }
            busy = false
        }
    }

    PhoneSurface {
        Text(
            text = "Connect to Ferrex",
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            modifier = Modifier.padding(top = 12.dp),
            text = when (state.reason) {
                NoServerReason.FirstInstall -> "Enter your server URL to start."
                NoServerReason.ResetConnection -> "Connection data was reset. Enter a server URL to continue."
                NoServerReason.ChangeServer -> "Choose a different Ferrex server. Your saved URL changes only after this check succeeds."
            },
            style = MaterialTheme.typography.bodyLarge,
        )
        state.previousServerUrl?.let {
            Text(
                modifier = Modifier.padding(top = 12.dp),
                text = "Current server: $it",
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        OutlinedTextField(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 28.dp),
            value = serverUrl,
            onValueChange = { serverUrl = it },
            label = { Text("Server URL, such as http://192.168.1.100:3000") },
            singleLine = true,
            enabled = !busy,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(onGo = { connect() }),
        )
        Button(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 20.dp),
            enabled = !busy && serverUrl.isNotBlank(),
            onClick = { connect() },
        ) {
            if (busy) CircularProgressIndicator(modifier = Modifier.padding(end = 12.dp))
            Text("Retry / Connect")
        }
        TextButton(
            modifier = Modifier.padding(top = 8.dp),
            onClick = onResetConnection,
        ) {
            Text("Reset connection")
        }
        message?.let {
            Text(
                modifier = Modifier.padding(top = 20.dp),
                text = it,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@Composable
fun PhoneLoginScreen(
    state: SessionState.NeedsLogin,
    onLogin: suspend (String, String) -> LoginResult,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    val isFatal = state.reason == LoginRequiredReason.SetupRequired ||
        state.reason == LoginRequiredReason.RegistrationClosed
    val scope = rememberCoroutineScope()
    var username by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var message by remember(state.reason) { mutableStateOf<String?>(loginReasonCopy(state.reason)) }
    var busy by remember { mutableStateOf(false) }

    fun login() {
        scope.launch {
            busy = true
            message = null
            when (val result = onLogin(username, password)) {
                is LoginResult.Success -> {
                    message = if (result.requiresPinSetup) {
                        "Signed in. This server requires PIN setup before PIN sign-in can be used."
                    } else {
                        "Signed in."
                    }
                }
                is LoginResult.Error -> message = result.message
            }
            busy = false
        }
    }

    PhoneSurface {
        Text(
            text = if (isFatal) "Server action required" else "Sign in",
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            modifier = Modifier.padding(top = 12.dp),
            text = "Current server: ${state.serverUrl}",
            style = MaterialTheme.typography.bodyMedium,
        )
        Text(
            modifier = Modifier.padding(top = 16.dp),
            text = if (isFatal) setupCopy(state.reason) else "Use device password sign-in for the current Ferrex auth API. PIN setup is reported honestly when the server requires it.",
            style = MaterialTheme.typography.bodyLarge,
        )
        if (!isFatal) {
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 28.dp),
                value = username,
                onValueChange = { username = it },
                label = { Text("Username") },
                singleLine = true,
                enabled = !busy,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
            )
            OutlinedTextField(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 14.dp),
                value = password,
                onValueChange = { password = it },
                label = { Text("Password") },
                singleLine = true,
                enabled = !busy,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { login() }),
            )
            Button(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 20.dp),
                enabled = !busy && username.isNotBlank() && password.isNotBlank(),
                onClick = { login() },
            ) {
                if (busy) CircularProgressIndicator(modifier = Modifier.padding(end = 12.dp))
                Text("Sign in")
            }
        }
        RecoveryActions(
            onRetry = onRetry,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
        )
        message?.let {
            Text(
                modifier = Modifier.padding(top = 20.dp),
                text = it,
                color = MaterialTheme.colorScheme.primary,
            )
        }
    }
}

@Composable
fun PhoneRecoverableScreen(
    state: SessionState.RecoverableFailure,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    PhoneSurface {
        Text(
            text = recoverableTitle(state.reason),
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            modifier = Modifier.padding(top = 12.dp),
            text = "Current server: ${state.serverUrl}",
            style = MaterialTheme.typography.bodyMedium,
        )
        Text(
            modifier = Modifier.padding(top = 16.dp),
            text = recoverableCopy(state.reason),
            style = MaterialTheme.typography.bodyLarge,
        )
        RecoveryActions(
            onRetry = onRetry,
            onSignOut = onSignOut,
            onChangeServer = onChangeServer,
            onResetConnection = onResetConnection,
        )
    }
}

@Composable
private fun PhoneSurface(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 28.dp, vertical = 44.dp),
            horizontalAlignment = Alignment.Start,
            verticalArrangement = Arrangement.Center,
            content = content,
        )
    }
}

@Composable
private fun RecoveryActions(
    includeRetry: Boolean = true,
    onRetry: () -> Unit,
    onSignOut: () -> Unit,
    onChangeServer: () -> Unit,
    onResetConnection: () -> Unit,
) {
    Spacer(Modifier.height(28.dp))
    if (includeRetry) {
        Button(onClick = onRetry, modifier = Modifier.fillMaxWidth()) {
            Text("Retry")
        }
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        TextButton(onClick = onSignOut, modifier = Modifier.weight(1f)) {
            Text("Sign out", textAlign = TextAlign.Center)
        }
        TextButton(onClick = onChangeServer, modifier = Modifier.weight(1f)) {
            Text("Change server", textAlign = TextAlign.Center)
        }
    }
    TextButton(onClick = onResetConnection, modifier = Modifier.fillMaxWidth()) {
        Text("Reset connection")
    }
}

private fun loginReasonCopy(reason: LoginRequiredReason): String? = when (reason) {
    LoginRequiredReason.NoSavedSession -> null
    LoginRequiredReason.SignedOut -> "Signed out locally. Server URL was preserved."
    LoginRequiredReason.SessionExpired -> "Session expired. Sign in again or choose a recovery action."
    LoginRequiredReason.SessionRevoked -> "Session was revoked or could not be refreshed. Sign in again."
    LoginRequiredReason.RefreshFailed -> "Refresh failed because the saved tokens were not usable. Sign in again."
    LoginRequiredReason.SetupRequired -> "This server still needs first-run setup."
    LoginRequiredReason.RegistrationClosed -> "This server is not accepting app account setup from Android."
    LoginRequiredReason.ChangedServer -> "Server changed. Sign in to continue."
}

private fun setupCopy(reason: LoginRequiredReason): String = when (reason) {
    LoginRequiredReason.SetupRequired -> "Complete Ferrex server setup with the server's supported setup path, then retry here. The Android app does not create an admin account."
    LoginRequiredReason.RegistrationClosed -> "An administrator must create or enable an account on this Ferrex server. The Android app does not expose a fake registration flow."
    else -> "Sign in is unavailable until the server is ready."
}

private fun recoverableTitle(reason: RecoverableFailureReason): String = when (reason) {
    RecoverableFailureReason.ServerUnreachable -> "Server unreachable"
    RecoverableFailureReason.ValidationUnavailable -> "Session validation unavailable"
    RecoverableFailureReason.RefreshUnavailable -> "Refresh unavailable"
    RecoverableFailureReason.InvalidServerResponse -> "Server response changed"
}

private fun recoverableCopy(reason: RecoverableFailureReason): String = when (reason) {
    RecoverableFailureReason.ServerUnreachable -> "Saved tokens and server URL were preserved. Protected screens are closed until Ferrex can validate the session."
    RecoverableFailureReason.ValidationUnavailable -> "Ferrex could not validate the restored session. Retry when the server is reachable, or recover locally."
    RecoverableFailureReason.RefreshUnavailable -> "Ferrex could not refresh the session right now. Tokens were preserved unless the server proved they were revoked."
    RecoverableFailureReason.InvalidServerResponse -> "Ferrex reached the server but could not understand the auth response. Retry after updating the server or recover locally."
}
