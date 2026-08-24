plugins {
    `kotlin-dsl`
    `java-gradle-plugin`
    `maven-publish`
}

group = "io.github.rabbittrix"
version = "0.1.0-alpha"

gradlePlugin {
    plugins {
        register("rustjvm") {
            id = "io.github.rabbittrix"
            implementationClass = "io.rustjvm.gradle.RustJVMPlugin"
        }
    }
}

publishing {
    publications {
        create<MavenPublication>("pluginMaven") {
            from(components["java"])
        }
    }
}
