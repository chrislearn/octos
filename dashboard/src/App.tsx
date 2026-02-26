import { Routes, Route } from 'react-router-dom'
import { ToastProvider } from './components/Toast'
import { AuthProvider } from './contexts/AuthContext'
import AuthGuard from './components/AuthGuard'
import AdminGuard from './components/AdminGuard'
import Layout from './components/Layout'
import Dashboard from './pages/Dashboard'
import ProfilePage from './pages/ProfilePage'
import NewProfile from './pages/NewProfile'
import Settings from './pages/Settings'
import LoginPage from './pages/LoginPage'
import MyProfile from './pages/MyProfile'
import UsersPage from './pages/UsersPage'

export default function App() {
  return (
    <AuthProvider>
      <ToastProvider>
        <Routes>
          <Route path="login" element={<LoginPage />} />
          <Route element={<AuthGuard><Layout /></AuthGuard>}>
            <Route index element={<Dashboard />} />
            <Route path="my-profile" element={<MyProfile />} />
            <Route path="settings" element={<Settings />} />
            <Route path="profiles/new" element={<AdminGuard><NewProfile /></AdminGuard>} />
            <Route path="profiles/:id" element={<AdminGuard><ProfilePage /></AdminGuard>} />
            <Route path="users" element={<AdminGuard><UsersPage /></AdminGuard>} />
          </Route>
        </Routes>
      </ToastProvider>
    </AuthProvider>
  )
}
