package com.ferrex.android.ui.auth

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrex.android.core.auth.AuthManager
import com.ferrex.android.core.auth.ConnectResult
import com.ferrex.android.core.auth.LoginResult
import com.ferrex.android.core.auth.SessionState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class AuthViewModel @Inject constructor(
    private val authManager: AuthManager,
) : ViewModel() {

    val sessionState: StateFlow<SessionState> = authManager.sessionState

    private val _connectUiState = MutableStateFlow(ConnectUiState())
    val connectUiState: StateFlow<ConnectUiState> = _connectUiState.asStateFlow()

    private val _loginUiState = MutableStateFlow(LoginUiState())
    val loginUiState: StateFlow<LoginUiState> = _loginUiState.asStateFlow()

    init {
        viewModelScope.launch {
            authManager.initialize()
        }
    }

    // ── Server connect ──────────────────────────────────────────────

    fun updateServerUrl(url: String) {
        _connectUiState.update { it.copy(serverUrl = url, error = null) }
    }

    fun connectToServer() {
        val url = _connectUiState.value.serverUrl.trim()
        if (url.isBlank()) {
            _connectUiState.update { it.copy(error = "Please enter a server URL") }
            return
        }

        viewModelScope.launch {
            _connectUiState.update { it.copy(isLoading = true, error = null) }
            when (val result = authManager.connectToServer(url)) {
                is ConnectResult.Success -> {
                    _connectUiState.update {
                        it.copy(
                            isLoading = false,
                            isConnected = true,
                            needsSetup = result.needsSetup,
                            registrationOpen = result.registrationOpen,
                        )
                    }
                }
                is ConnectResult.Error -> {
                    _connectUiState.update {
                        it.copy(isLoading = false, error = result.message)
                    }
                }
            }
        }
    }

    // ── Login ───────────────────────────────────────────────────────

    fun updateUsername(username: String) {
        _loginUiState.update { it.copy(username = username, error = null) }
    }

    fun updatePassword(password: String) {
        _loginUiState.update { it.copy(password = password, error = null) }
    }

    fun login() {
        val state = _loginUiState.value
        if (state.username.isBlank() || state.password.isBlank()) {
            _loginUiState.update { it.copy(error = "Username and password are required") }
            return
        }

        viewModelScope.launch {
            _loginUiState.update { it.copy(isLoading = true, error = null) }
            when (val result = authManager.login(state.username, state.password)) {
                is LoginResult.Success -> {
                    _loginUiState.update { it.copy(isLoading = false) }
                }
                is LoginResult.InvalidCredentials -> {
                    _loginUiState.update {
                        it.copy(isLoading = false, error = "Invalid username or password")
                    }
                }
                is LoginResult.Error -> {
                    _loginUiState.update {
                        it.copy(isLoading = false, error = result.message)
                    }
                }
            }
        }
    }
}

data class ConnectUiState(
    val serverUrl: String = "",
    val isLoading: Boolean = false,
    val isConnected: Boolean = false,
    val needsSetup: Boolean = false,
    val registrationOpen: Boolean = false,
    val error: String? = null,
)

data class LoginUiState(
    val username: String = "",
    val password: String = "",
    val isLoading: Boolean = false,
    val error: String? = null,
)
