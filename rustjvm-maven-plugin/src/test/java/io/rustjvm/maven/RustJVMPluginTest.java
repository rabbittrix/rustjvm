package io.rustjvm.maven;

import org.junit.Test;

import java.io.File;
import java.util.List;

import static org.junit.Assert.*;

public class RustJVMPluginTest {

    private RustJVMRunMojo runMojo(int port, String src) {
        RustJVMRunMojo mojo = new RustJVMRunMojo();
        mojo.port = port;
        mojo.sourceDir = new File(src);
        return mojo;
    }

    @Test
    public void runGoalGeneratesValidCommand() {
        RustJVMRunMojo mojo = runMojo(8080, "src/main/java");
        List<String> cmd = mojo.buildCommand(mojo.runArgs());

        assertEquals("rustjvm", cmd.get(0));
        int portFlag = cmd.indexOf("--port");
        assertTrue("--port present", portFlag >= 0);
        assertEquals("8080", cmd.get(portFlag + 1));
        int srcFlag = cmd.indexOf("--src");
        assertTrue("--src present", srcFlag >= 0);
        assertEquals(new File("src/main/java").getPath(), cmd.get(srcFlag + 1));
    }

    @Test
    public void runGoalRespectsCustomPort() {
        RustJVMRunMojo mojo = runMojo(9090, "src");
        List<String> args = mojo.runArgs();
        assertEquals("9090", args.get(args.indexOf("--port") + 1));
    }

    @Test
    public void buildGoalInvokesBuildSubcommand() {
        RustJVMBuildMojo mojo = new RustJVMBuildMojo();
        mojo.sourceDir = new File("src");
        List<String> args = mojo.buildArgs();
        assertEquals("build", args.get(0));
        assertTrue(args.contains("--src"));
    }

    @Test
    public void packageGoalWritesMinimalDockerfile() {
        RustJVMPackageMojo mojo = new RustJVMPackageMojo();
        mojo.port = 8080;
        String dockerfile = mojo.dockerfile();
        assertTrue(dockerfile.contains("FROM debian:bookworm-slim"));
        assertTrue(dockerfile.contains("EXPOSE 8080"));
        assertTrue(dockerfile.contains("HEALTHCHECK"));
    }
}
