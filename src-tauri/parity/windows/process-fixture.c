// Deliberately small Windows process oracle fixture: never a shell or real application.
#include <windows.h>
#include <wchar.h>
#include <stdlib.h>
#include <string.h>
static void out(HANDLE h, const char *s, DWORD n) { DWORD written; WriteFile(h, s, n, &written, NULL); }
int wmain(int argc, wchar_t **argv) {
  if (argc > 1 && !wcscmp(argv[1], L"hold")) { Sleep(800); return 0; }
  if (argc > 1 && !wcscmp(argv[1], L"orphan")) {
    wchar_t exe[32768], cmd[32768]; GetModuleFileNameW(NULL, exe, 32768);
    _snwprintf(cmd, 32768, L"\"%ls\" hold", exe);
    STARTUPINFOW startup = {0}; PROCESS_INFORMATION child = {0};
    startup.cb = sizeof(startup); startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    if (!CreateProcessW(exe, cmd, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &child)) return 3;
    CloseHandle(child.hThread); CloseHandle(child.hProcess); return 0;
  }
  if (argc > 1 && !wcscmp(argv[1], L"wait")) { Sleep(60000); return 0; }
  if (argc > 2 && !wcscmp(argv[1], L"exit")) ExitProcess((DWORD)wcstoul(argv[2], NULL, 10));
  if (argc > 1 && !wcscmp(argv[1], L"stderr")) { out(GetStdHandle(STD_ERROR_HANDLE), "problem\n", 8); return 2; }
  if (argc > 1 && !wcscmp(argv[1], L"binary")) { const char b[]={ 'a', (char)0xff, (char)0xfe, 'b' }; out(GetStdHandle(STD_OUTPUT_HANDLE), b, 4); return 0; }
  if (argc > 1 && !wcscmp(argv[1], L"big")) {
    char b[8192]; memset(b, 'x', sizeof b);
    for (int i=0;i<140;i++) out(GetStdHandle(STD_OUTPUT_HANDLE), b, sizeof b);
    return 0;
  }
  // Observe argv[0] and the raw command line as well as CRT-decoded arguments.
  for (int i=-1;i<argc;i++) {
    const wchar_t *s = i < 0 ? GetCommandLineW() : argv[i];
    int n = WideCharToMultiByte(CP_UTF8, 0, s, -1, NULL, 0, NULL, NULL);
    char *b = malloc(n); WideCharToMultiByte(CP_UTF8, 0, s, -1, b, n, NULL, NULL);
    out(GetStdHandle(STD_OUTPUT_HANDLE), b, n-1); out(GetStdHandle(STD_OUTPUT_HANDLE), "\n", 1); free(b);
  }
  return 0;
}
