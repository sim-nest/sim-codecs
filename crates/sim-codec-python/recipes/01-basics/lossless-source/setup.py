match message:
    case {"value": value}:
        rendered = f"value={value!r}"
