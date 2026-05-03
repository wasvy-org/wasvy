from enum import Enum
import json

import msgpack
from wit_world.imports.app import Serialize

class SerializeType(Enum):
    JSON = "json"
    MSGPACK = "msgpack"
    UNKOWN = "unknown"

class WasvyCodec():
    def __init__(self):
        serialize_type = Serialize().get_type()
        if serialize_type == "json":
            self.serialize_type = SerializeType.JSON
        elif serialize_type == "msgpack":
            self.serialize_type = SerializeType.MSGPACK
        else:
            raise ValueError(f"Unsupported serialize type: {serialize_type}")

    def loads(self, data):
        if self.serialize_type == SerializeType.JSON:
            return json.loads(data)
        elif self.serialize_type == SerializeType.MSGPACK:
            return msgpack.loads(data)
        
    def dumps(self, obj):
        if self.serialize_type == SerializeType.JSON:
            return json.dumps(obj).encode('utf-8')
        elif self.serialize_type == SerializeType.MSGPACK:
            return msgpack.dumps(obj, use_bin_type=True)
        

_codec_instance: WasvyCodec | None = None


def get_codec() -> WasvyCodec:
    global _codec_instance
    if _codec_instance is None:
        _codec_instance = WasvyCodec()
    return _codec_instance