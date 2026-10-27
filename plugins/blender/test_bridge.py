"""Gerçek Blender sürecinde eklenti kaydı ve .blend içe aktarma testi."""

import hashlib
import importlib.util
import sys
import tempfile
from pathlib import Path

import bpy


plugin_path = Path(__file__).with_name("opendam_bridge.py")
spec = importlib.util.spec_from_file_location("opendam_bridge", plugin_path)
bridge = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = bridge
spec.loader.exec_module(bridge)
bridge.register()
try:
    with tempfile.TemporaryDirectory(prefix="lumiadam-blender-test-") as directory:
        asset = Path(directory) / "test-asset.blend"
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        bpy.ops.mesh.primitive_cube_add()
        bpy.context.object.name = "LumiaDAM_Test_Cube"
        bpy.data.libraries.write(str(asset), {bpy.context.object})
        before = hashlib.sha256(asset.read_bytes()).hexdigest()
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        assert bpy.ops.opendam.import_asset(file_path=str(asset)) == {"FINISHED"}
        imported = bpy.context.scene.objects.get("LumiaDAM_Test_Cube")
        assert imported is not None and imported.type == "MESH"
        assert len(imported.data.vertices) == 8
        assert hashlib.sha256(asset.read_bytes()).hexdigest() == before
        print(f"PASS: Blender {bpy.app.version_string}; plugin registration, .blend import, source preserved")
finally:
    bridge.unregister()
