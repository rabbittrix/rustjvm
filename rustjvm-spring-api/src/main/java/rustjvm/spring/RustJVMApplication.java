package rustjvm.spring;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;
import rustjvm.spring.context.ComponentScan;

/** Entry-point marker, mirroring @SpringBootApplication. Meta-annotated with
 *  @ComponentScan: scanning defaults to the annotated class's package. */
@Target(ElementType.TYPE)
@Retention(RetentionPolicy.RUNTIME)
@ComponentScan
public @interface RustJVMApplication {
}
