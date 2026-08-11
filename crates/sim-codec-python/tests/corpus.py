@decorator
class Box[T: object]:
    """trivia and locations stay intact"""
    def map[U](self, fn: callable[[T], U], /, *, strict=True) -> U:
        return fn(self.value)

async def collect(items):
    return [await item async for item in items if item is not None]

match payload:
    case {"kind": "point", "xy": [x, y], **rest} if x >= 0:
        result := (x + y * 2)
    case Box(value=v) | [v]:
        pass

try:
    raise ExceptionGroup("many", [])
except* ValueError as errors:
    print(f"errors={errors!r:20}")
finally:
    template = t"result={result}"
