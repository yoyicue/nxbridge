import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.MotionEvent;
import java.lang.reflect.Method;

public final class MultiTouchProbe {
    public static void main(String[] args) throws Exception {
        Class<?> cls = Class.forName("android.hardware.input.InputManager");
        Object manager = cls.getMethod("getInstance").invoke(null);
        Method inject = cls.getMethod("injectInputEvent", InputEvent.class, int.class);
        MotionEvent.PointerProperties[] pp = new MotionEvent.PointerProperties[2];
        MotionEvent.PointerCoords[] pc = new MotionEvent.PointerCoords[2];
        for (int i = 0; i < 2; i++) { pp[i] = new MotionEvent.PointerProperties(); pp[i].id = i; pp[i].toolType = MotionEvent.TOOL_TYPE_FINGER; pc[i] = new MotionEvent.PointerCoords(); pc[i].pressure = 1; pc[i].size = 1; }
        pc[0].x=410; pc[0].y=365; pc[1].x=2192; pc[1].y=560;
        long down=SystemClock.uptimeMillis();
        send(inject,manager,down,MotionEvent.ACTION_DOWN,1,pp,pc);
        pc[0].y=180; send(inject,manager,down,MotionEvent.ACTION_MOVE,1,pp,pc);
        send(inject,manager,down,MotionEvent.ACTION_POINTER_DOWN | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT),2,pp,pc);
        Thread.sleep(500);
        send(inject,manager,down,MotionEvent.ACTION_POINTER_UP | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT),2,pp,pc);
        send(inject,manager,down,MotionEvent.ACTION_UP,1,pp,pc);
    }
    static void send(Method inject,Object manager,long down,int action,int count,MotionEvent.PointerProperties[] pp,MotionEvent.PointerCoords[] pc) throws Exception {
        MotionEvent e=MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,count,pp,pc,0,0,1,1,0,0,InputDevice.SOURCE_TOUCHSCREEN,0);
        if (!((Boolean)inject.invoke(manager,e,2))) throw new RuntimeException("inject failed");
        e.recycle();
    }
}
