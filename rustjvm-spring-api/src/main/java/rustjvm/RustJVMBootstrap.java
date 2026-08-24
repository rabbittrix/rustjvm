package rustjvm;

import java.io.File;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import rustjvm.spring.RustJVMApplication;

/**
 * Bootstrap entry point for RustJVM applications.
 *
 * <p>The Rust runtime does the real work; this class only validates the
 * application class, locates the {@code rustjvm} binary, and delegates:
 *
 * <pre>{@code
 * public static void main(String[] args) {
 *     System.exit(RustJVMBootstrap.run(MyApp.class, args));
 * }
 * }</pre>
 */
public final class RustJVMBootstrap {

    private RustJVMBootstrap() {}

    /**
     * Start the RustJVM runtime with the given application class.
     *
     * @param appClass a class annotated with {@link RustJVMApplication}
     * @param args     extra arguments forwarded to the runtime
     * @return the runtime's exit code
     * @throws IllegalArgumentException if the class lacks @RustJVMApplication
     * @throws IllegalStateException    if no rustjvm binary can be located
     */
    public static int run(Class<?> appClass, String[] args) {
        if (!appClass.isAnnotationPresent(RustJVMApplication.class)) {
            throw new IllegalArgumentException(
                "Class " + appClass.getName() + " must be annotated with @RustJVMApplication");
        }

        String binary = findRustJVMBinary(
            System.getenv("RUSTJVM_HOME"),
            System.getProperty("user.home"),
            System.getenv("PATH"));

        List<String> command = buildCommand(
            appClass, binary, detectSourceRoot(Path.of("").toAbsolutePath()), args);

        ProcessBuilder pb = new ProcessBuilder(command).inheritIO();
        try {
            return pb.start().waitFor();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new IllegalStateException("Interrupted while waiting for the RustJVM runtime", e);
        } catch (java.io.IOException e) {
            throw new IllegalStateException("Failed to start the RustJVM runtime: " + binary, e);
        }
    }

    /** rustjvm run --src <source-root> [--main <fqcn>] [args...] */
    static List<String> buildCommand(Class<?> appClass, String binary, String sourceRoot, String[] args) {
        List<String> command = new ArrayList<>();
        command.add(binary);
        command.add("run");
        command.add("--src");
        command.add(sourceRoot);
        command.add("--main");
        command.add(appClass.getName());
        command.addAll(List.of(args));
        return command;
    }

    /**
     * Locate the runtime binary, in priority order:
     * RUSTJVM_HOME/bin, ~/.cargo/bin, then every directory on PATH.
     */
    static String findRustJVMBinary(String rustjvmHome, String userHome, String path) {
        if (rustjvmHome != null && !rustjvmHome.isBlank()) {
            File candidate = new File(new File(rustjvmHome, "bin"), binaryName());
            if (isExecutableFile(candidate)) {
                return candidate.getAbsolutePath();
            }
        }

        if (userHome != null) {
            File candidate = new File(new File(userHome, ".cargo/bin"), binaryName());
            if (isExecutableFile(candidate)) {
                return candidate.getAbsolutePath();
            }
        }

        if (path != null) {
            for (String dir : path.split(File.pathSeparator)) {
                File candidate = new File(dir, binaryName());
                if (isExecutableFile(candidate)) {
                    return candidate.getAbsolutePath();
                }
            }
        }

        throw new IllegalStateException(
            "RustJVM runtime not found.\n"
                + "Install it with: cargo install rustjvm-cli\n"
                + "or run:        curl -fsSL https://rustjvm.dev/install.sh | bash\n"
                + "or set the RUSTJVM_HOME environment variable.");
    }

    /**
     * Best-effort source root for a Maven/Gradle-style project: prefer
     * {@code src/main/java}, fall back to {@code src}, then the working dir.
     */
    static String detectSourceRoot(Path workingDir) {
        Path mavenLayout = workingDir.resolve("src/main/java");
        if (Files.isDirectory(mavenLayout)) {
            return mavenLayout.toString();
        }
        Path plain = workingDir.resolve("src");
        if (Files.isDirectory(plain)) {
            return plain.toString();
        }
        return workingDir.toString();
    }

    static String binaryName() {
        return isWindows() ? "rustjvm.exe" : "rustjvm";
    }

    private static boolean isWindows() {
        return System.getProperty("os.name", "").toLowerCase().contains("win");
    }

    private static boolean isExecutableFile(File candidate) {
        return candidate.isFile() && candidate.canExecute();
    }
}
