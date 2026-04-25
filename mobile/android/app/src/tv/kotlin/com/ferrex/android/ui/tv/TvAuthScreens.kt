package com.ferrex.android.ui.tv

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.ferrex.android.core.auth.SessionState
import com.ferrex.android.ui.auth.AuthViewModel

@Composable
fun TvServerConnectScreen(
    viewModel: AuthViewModel,
    onConnected: () -> Unit,
) {
    val uiState by viewModel.connectUiState.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val serverFocusRequester = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        runCatching { serverFocusRequester.requestFocus() }
    }

    LaunchedEffect(uiState.isConnected) {
        if (uiState.isConnected) onConnected()
    }

    LaunchedEffect(uiState.error) {
        uiState.error?.let { snackbarHostState.showSnackbar(it) }
    }

    TvAuthSurface(snackbarHostState = snackbarHostState) {
        Text(
            text = "Connect to Ferrex",
            style = MaterialTheme.typography.displaySmall,
            color = Color.White,
            fontWeight = FontWeight.Bold,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            text = "Enter the server address for this TV.",
            style = MaterialTheme.typography.titleLarge,
            color = Color.White.copy(alpha = 0.72f),
        )
        Spacer(Modifier.height(36.dp))
        OutlinedTextField(
            value = uiState.serverUrl,
            onValueChange = viewModel::updateServerUrl,
            label = { Text("Server URL") },
            placeholder = { Text("http://192.168.1.100:3000") },
            singleLine = true,
            enabled = !uiState.isLoading,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Go,
            ),
            keyboardActions = KeyboardActions(
                onGo = { viewModel.connectToServer() },
            ),
            colors = tvTextFieldColors(),
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(serverFocusRequester)
                .semantics { contentDescription = "Server URL" },
        )
        Spacer(Modifier.height(24.dp))
        Button(
            onClick = viewModel::connectToServer,
            enabled = !uiState.isLoading && uiState.serverUrl.isNotBlank(),
            modifier = Modifier.fillMaxWidth(),
        ) {
            TvButtonProgress(visible = uiState.isLoading)
            Text("Connect")
        }
    }
}

@Composable
fun TvLoginScreen(
    viewModel: AuthViewModel,
    onLoginSuccess: () -> Unit,
    onNavigateToRegister: () -> Unit,
) {
    val uiState by viewModel.loginUiState.collectAsState()
    val sessionState by viewModel.sessionState.collectAsState()
    val snackbarHostState = remember { SnackbarHostState() }
    val usernameFocusRequester = remember { FocusRequester() }
    val passwordFocusRequester = remember { FocusRequester() }
    var passwordVisible by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        runCatching { usernameFocusRequester.requestFocus() }
    }

    LaunchedEffect(sessionState) {
        if (sessionState is SessionState.Authenticated) onLoginSuccess()
    }

    LaunchedEffect(uiState.error) {
        uiState.error?.let { snackbarHostState.showSnackbar(it) }
    }

    TvAuthSurface(snackbarHostState = snackbarHostState) {
        Text(
            text = "Sign in",
            style = MaterialTheme.typography.displaySmall,
            color = Color.White,
            fontWeight = FontWeight.Bold,
        )
        Spacer(Modifier.height(12.dp))
        Text(
            text = "Use your Ferrex account on this server.",
            style = MaterialTheme.typography.titleLarge,
            color = Color.White.copy(alpha = 0.72f),
        )
        Spacer(Modifier.height(36.dp))
        OutlinedTextField(
            value = uiState.username,
            onValueChange = viewModel::updateUsername,
            label = { Text("Username") },
            singleLine = true,
            enabled = !uiState.isLoading,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
            keyboardActions = KeyboardActions(onNext = { passwordFocusRequester.requestFocus() }),
            colors = tvTextFieldColors(),
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(usernameFocusRequester)
                .semantics { contentDescription = "Username" },
        )
        Spacer(Modifier.height(18.dp))
        OutlinedTextField(
            value = uiState.password,
            onValueChange = viewModel::updatePassword,
            label = { Text("Password") },
            singleLine = true,
            enabled = !uiState.isLoading,
            visualTransformation = if (passwordVisible) VisualTransformation.None else PasswordVisualTransformation(),
            trailingIcon = {
                IconButton(onClick = { passwordVisible = !passwordVisible }) {
                    Icon(
                        imageVector = if (passwordVisible) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                        contentDescription = if (passwordVisible) "Hide password" else "Show password",
                    )
                }
            },
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Go,
            ),
            keyboardActions = KeyboardActions(onGo = { viewModel.login() }),
            colors = tvTextFieldColors(),
            modifier = Modifier
                .fillMaxWidth()
                .focusRequester(passwordFocusRequester)
                .semantics { contentDescription = "Password" },
        )
        Spacer(Modifier.height(24.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Button(
                onClick = viewModel::login,
                enabled = !uiState.isLoading && uiState.username.isNotBlank() && uiState.password.isNotBlank(),
                modifier = Modifier.weight(1f),
            ) {
                TvButtonProgress(visible = uiState.isLoading)
                Text("Sign in")
            }
            TextButton(
                onClick = onNavigateToRegister,
                modifier = Modifier.weight(1f),
            ) {
                Text("Create account")
            }
        }
    }
}

@Composable
private fun TvAuthSurface(
    snackbarHostState: SnackbarHostState,
    content: @Composable ColumnScope.() -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF070A12))
            .windowInsetsPadding(WindowInsets.safeDrawing),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        listOf(Color(0xFF172554), Color(0xFF070A12), Color(0xFF020617)),
                    ),
                ),
        )
        Surface(
            color = Color.Black.copy(alpha = 0.24f),
            shape = MaterialTheme.shapes.extraLarge,
            tonalElevation = 0.dp,
            modifier = Modifier
                .align(Alignment.Center)
                .width(560.dp),
        ) {
            Column(
                modifier = Modifier.padding(40.dp),
                horizontalAlignment = Alignment.Start,
                content = content,
            )
        }
        SnackbarHost(
            hostState = snackbarHostState,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
    }
}

@Composable
private fun TvButtonProgress(visible: Boolean) {
    AnimatedVisibility(visible = visible) {
        CircularProgressIndicator(
            modifier = Modifier
                .size(20.dp)
                .padding(end = 8.dp),
            strokeWidth = 2.dp,
            color = MaterialTheme.colorScheme.onPrimary,
        )
    }
}

@Composable
private fun tvTextFieldColors() = OutlinedTextFieldDefaults.colors(
    focusedTextColor = Color.White,
    unfocusedTextColor = Color.White,
    focusedLabelColor = Color.White,
    unfocusedLabelColor = Color.White.copy(alpha = 0.72f),
    focusedBorderColor = MaterialTheme.colorScheme.primary,
    unfocusedBorderColor = Color.White.copy(alpha = 0.42f),
    focusedContainerColor = Color.White.copy(alpha = 0.08f),
    unfocusedContainerColor = Color.White.copy(alpha = 0.06f),
    cursorColor = Color.White,
)
