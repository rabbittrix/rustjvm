package rustjvm;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertThrows;
import static org.junit.Assert.assertTrue;

import java.io.File;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.Test;
import rustjvm.spring.RustJVMApplication;

public class RustJVMBootstrapTest {

    @RustJVMApplication
    static class FakeApp {}

    @Test
    public void bootstrapValidatesAnnotation() {
        assertThrows(IllegalArgumentException.class, () -> {
            RustJVMBootstrap.run(String.class, new String[0]);
        });
    }

    @Test
    public void bootstrapFindsRustJVMBinaryInRustjvmHome() throws Exception {
        Path home = Files.createTempDirectory("rustjvm-home");
        Path bin = Files.createDirectories(home.resolve("bin"));
        File fake = Files.createFile(bin.resolve(RustJVMBootstrap.binaryName())).toFile();
        fake.setExecutable(true);

        String found = RustJVMBootstrap.findRustJVMBinary(
            home.toString(), "/nonexistent-user-home", "");
        assertEquals(fake.getAbsolutePath(), found);
    }

    @Test
    public void bootstrapFallsBackToPath() throws Exception {
        Path pathDir = Files.createTempDirectory("rustjvm-path");
        File fake = Files.createFile(pathDir.resolve(RustJVMBootstrap.binaryName())).toFile();
        fake.setExecutable(true);

        String found = RustJVMBootstrap.findRustJVMBinary(
            null, "/nonexistent-user-home", pathDir.toString());
        assertEquals(fake.getAbsolutePath(), found);
    }

    @Test
    public void bootstrapFailsWithInstallInstructions() {
        IllegalStateException e = assertThrows(IllegalStateException.class, () -> {
            RustJVMBootstrap.findRustJVMBinary(null, "/nonexistent-user-home", "");
        });
        assertTrue(e.getMessage().contains("cargo install rustjvm-cli"));
    }

    @Test
    public void bootstrapBuildsCorrectCommand() {
        List<String> cmd = RustJVMBootstrap.buildCommand(
            FakeApp.class, "/usr/local/bin/rustjvm", "src/main/java", new String[] {"--port", "9090"});

        assertEquals("/usr/local/bin/rustjvm", cmd.get(0));
        assertEquals("run", cmd.get(1));
        assertEquals("--src", cmd.get(2));
        assertEquals("src/main/java", cmd.get(3));
        assertEquals("--main", cmd.get(4));
        assertEquals(FakeApp.class.getName(), cmd.get(5));
        assertEquals("--port", cmd.get(6));
        assertEquals("9090", cmd.get(7));
    }

    @Test
    public void jarContainsAllAnnotations() {
        // Resolved through the classpath, so this passes against
        // target/classes now and against the packaged JAR later.
        String[] expected = {
            "rustjvm/spring/RustJVMApplication.class",
            "rustjvm/RustJVMBootstrap.class",
            "rustjvm/spring/context/Autowired.class",
            "rustjvm/spring/context/Bean.class",
            "rustjvm/spring/context/Component.class",
            "rustjvm/spring/context/ComponentScan.class",
            "rustjvm/spring/context/Configuration.class",
            "rustjvm/spring/context/Scope.class",
            "rustjvm/spring/context/Service.class",
            "rustjvm/spring/web/RestController.class",
            "rustjvm/spring/web/GetMapping.class",
            "rustjvm/spring/web/PostMapping.class",
            "rustjvm/spring/web/RequestMapping.class",
            "rustjvm/spring/web/RequestParam.class",
            "rustjvm/spring/ai/Prompt.class",
        };
        for (String entry : expected) {
            assertNotNull(
                "missing from the API JAR: " + entry,
                getClass().getClassLoader().getResource(entry));
        }
    }
}
