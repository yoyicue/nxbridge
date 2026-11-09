import com.limelight.binding.input.virtual_controller.StrictLayoutJson;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import org.json.JSONException;
import org.json.JSONObject;

/** Run on Android with app_process; exercises the platform JsonReader and JSONObject. */
public final class StrictLayoutJsonAndroidTest {
    private static byte[] bytes(String s) { return s.getBytes(StandardCharsets.UTF_8); }
    private static void reject(String label, byte[] data) throws Exception {
        try { StrictLayoutJson.parse(data); throw new AssertionError("accepted " + label); }
        catch (IOException | JSONException expected) { }
    }
    public static void main(String[] args) throws Exception {
        JSONObject good = StrictLayoutJson.parse(bytes("{\"schema\":1,\"identity\":\"布局\",\"controls\":[{\"id\":\"2\",\"x\":0.25}],\"revision\":1}"));
        if (!"布局".equals(good.getString("identity")) || !StrictLayoutJson.isInteger(good.opt("schema"))) throw new AssertionError("valid JSON");
        reject("duplicate root field", bytes("{\"schema\":1,\"schema\":2}"));
        reject("escaped duplicate field", bytes("{\"id\":1,\"\\u0069d\":2}"));
        reject("duplicate nested field", bytes("{\"controls\":[{\"x\":0.1,\"x\":0.2}]}"));
        reject("single quotes", bytes("{'schema':1}"));
        reject("comments", bytes("{\"schema\":1/* comment */}"));
        reject("trailing data", bytes("{\"schema\":1} true"));
        reject("not object", bytes("[1,2]"));
        reject("invalid UTF-8", new byte[] {'{', '"', (byte)0xc3, (byte)0x28, '"', ':', '1', '}'});
        reject("too large", new byte[65537]);
        StringBuilder deep = new StringBuilder("{\"v\":");
        for (int i = 0; i < 18; i++) deep.append('[');
        deep.append('0'); for (int i = 0; i < 18; i++) deep.append(']'); deep.append('}');
        reject("nesting", bytes(deep.toString()));
        StringBuilder many = new StringBuilder("{\"v\":[");
        for (int i = 0; i < 4100; i++) { if (i > 0) many.append(','); many.append('0'); }
        many.append("]}"); reject("too many values", bytes(many.toString()));
        if (StrictLayoutJson.isInteger("1") || StrictLayoutJson.isInteger(1.0) ||
            StrictLayoutJson.isNumber("0.25") || !StrictLayoutJson.isInteger(1L) || !StrictLayoutJson.isNumber(.5))
            throw new AssertionError("numeric coercion");
        System.out.println("PASS: strict Android JSON syntax, duplicate keys, bounds and numeric types");
    }
}
