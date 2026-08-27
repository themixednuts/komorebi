using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

internal static class DesktopSlideshow
{
    private static readonly Guid ShellItemId = new Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE");
    private static readonly Guid ShellItemArrayId = new Guid("B63EA76D-1F85-456F-A19C-48159EFA858B");

    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            if (args.Length == 1 && string.Equals(args[0], "get", StringComparison.OrdinalIgnoreCase))
            {
                foreach (string path in GetSlideshowItems())
                {
                    Console.WriteLine(path);
                }
                return 0;
            }

            if (args.Length == 2 && string.Equals(args[0], "set", StringComparison.OrdinalIgnoreCase))
            {
                string folder = Path.GetFullPath(args[1]);
                if (!Directory.Exists(folder))
                {
                    Console.Error.WriteLine("Slideshow folder does not exist: " + folder);
                    return 2;
                }

                SetSlideshowFolder(folder);
                foreach (string path in GetSlideshowItems())
                {
                    Console.WriteLine(path);
                }
                return 0;
            }

            Console.Error.WriteLine("Usage: DesktopSlideshow.exe get | set <folder>");
            return 2;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.ToString());
            return 1;
        }
    }

    private static void SetSlideshowFolder(string folder)
    {
        IntPtr shellItemPointer = IntPtr.Zero;
        IntPtr arrayPointer = IntPtr.Zero;
        IDesktopWallpaper wallpaper = null;
        object arrayObject = null;

        try
        {
            Guid shellItemId = ShellItemId;
            Guid shellItemArrayId = ShellItemArrayId;
            ThrowIfFailed(SHCreateItemFromParsingName(folder, IntPtr.Zero, ref shellItemId, out shellItemPointer));
            ThrowIfFailed(SHCreateShellItemArrayFromShellItem(shellItemPointer, ref shellItemArrayId, out arrayPointer));
            arrayObject = Marshal.GetObjectForIUnknown(arrayPointer);
            wallpaper = (IDesktopWallpaper)new DesktopWallpaperClass();
            ThrowIfFailed(wallpaper.SetSlideshow((IShellItemArray)arrayObject));
            ThrowIfFailed(wallpaper.SetSlideshowOptions(DesktopSlideshowOptions.ShuffleImages, 600000));
        }
        finally
        {
            if (wallpaper != null)
            {
                Marshal.FinalReleaseComObject(wallpaper);
            }
            if (arrayObject != null)
            {
                Marshal.FinalReleaseComObject(arrayObject);
            }
            if (arrayPointer != IntPtr.Zero)
            {
                Marshal.Release(arrayPointer);
            }
            if (shellItemPointer != IntPtr.Zero)
            {
                Marshal.Release(shellItemPointer);
            }
        }
    }

    private static IEnumerable<string> GetSlideshowItems()
    {
        IDesktopWallpaper wallpaper = null;
        IShellItemArray items = null;
        var paths = new List<string>();

        try
        {
            wallpaper = (IDesktopWallpaper)new DesktopWallpaperClass();
            ThrowIfFailed(wallpaper.GetSlideshow(out items));
            uint count;
            ThrowIfFailed(items.GetCount(out count));

            for (uint index = 0; index < count; index++)
            {
                IShellItem item;
                ThrowIfFailed(items.GetItemAt(index, out item));
                try
                {
                    IntPtr displayName;
                    ThrowIfFailed(item.GetDisplayName(ShellDisplayName.FileSystemPath, out displayName));
                    try
                    {
                        paths.Add(Marshal.PtrToStringUni(displayName));
                    }
                    finally
                    {
                        Marshal.FreeCoTaskMem(displayName);
                    }
                }
                finally
                {
                    Marshal.FinalReleaseComObject(item);
                }
            }
        }
        finally
        {
            if (items != null)
            {
                Marshal.FinalReleaseComObject(items);
            }
            if (wallpaper != null)
            {
                Marshal.FinalReleaseComObject(wallpaper);
            }
        }

        return paths;
    }

    private static void ThrowIfFailed(int result)
    {
        if (result < 0)
        {
            Marshal.ThrowExceptionForHR(result);
        }
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = true)]
    private static extern int SHCreateItemFromParsingName(
        string path,
        IntPtr bindContext,
        ref Guid interfaceId,
        out IntPtr shellItem);

    [DllImport("shell32.dll", PreserveSig = true)]
    private static extern int SHCreateShellItemArrayFromShellItem(
        IntPtr shellItem,
        ref Guid interfaceId,
        out IntPtr shellItemArray);

    [ComImport]
    [Guid("C2CF3110-460E-4FC1-B9D0-8A1C0C9CC4BD")]
    private class DesktopWallpaperClass
    {
    }

    [ComImport]
    [Guid("B92B56A9-8B55-4E14-9A89-0199BBB6F93B")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IDesktopWallpaper
    {
        [PreserveSig] int SetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitorId, [MarshalAs(UnmanagedType.LPWStr)] string wallpaper);
        [PreserveSig] int GetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitorId, out IntPtr wallpaper);
        [PreserveSig] int GetMonitorDevicePathAt(uint monitorIndex, out IntPtr monitorId);
        [PreserveSig] int GetMonitorDevicePathCount(out uint count);
        [PreserveSig] int GetMonitorRect([MarshalAs(UnmanagedType.LPWStr)] string monitorId, out NativeRect displayRect);
        [PreserveSig] int SetBackgroundColor(uint color);
        [PreserveSig] int GetBackgroundColor(out uint color);
        [PreserveSig] int SetPosition(DesktopWallpaperPosition position);
        [PreserveSig] int GetPosition(out DesktopWallpaperPosition position);
        [PreserveSig] int SetSlideshow([MarshalAs(UnmanagedType.Interface)] IShellItemArray items);
        [PreserveSig] int GetSlideshow([MarshalAs(UnmanagedType.Interface)] out IShellItemArray items);
        [PreserveSig] int SetSlideshowOptions(DesktopSlideshowOptions options, uint slideshowTick);
        [PreserveSig] int GetSlideshowOptions(out DesktopSlideshowOptions options, out uint slideshowTick);
        [PreserveSig] int AdvanceSlideshow([MarshalAs(UnmanagedType.LPWStr)] string monitorId, DesktopSlideshowDirection direction);
        [PreserveSig] int GetStatus(out DesktopSlideshowState state);
        [PreserveSig] int Enable([MarshalAs(UnmanagedType.Bool)] bool enable);
    }

    [ComImport]
    [Guid("B63EA76D-1F85-456F-A19C-48159EFA858B")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellItemArray
    {
        [PreserveSig] int BindToHandler(IntPtr bindContext, ref Guid handlerId, ref Guid interfaceId, out IntPtr result);
        [PreserveSig] int GetPropertyStore(int flags, ref Guid interfaceId, out IntPtr result);
        [PreserveSig] int GetPropertyDescriptionList(IntPtr propertyKey, ref Guid interfaceId, out IntPtr result);
        [PreserveSig] int GetAttributes(uint flags, uint mask, out uint attributes);
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int GetItemAt(uint index, [MarshalAs(UnmanagedType.Interface)] out IShellItem item);
        [PreserveSig] int EnumItems(out IntPtr enumerator);
    }

    [ComImport]
    [Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellItem
    {
        [PreserveSig] int BindToHandler(IntPtr bindContext, ref Guid handlerId, ref Guid interfaceId, out IntPtr result);
        [PreserveSig] int GetParent([MarshalAs(UnmanagedType.Interface)] out IShellItem parent);
        [PreserveSig] int GetDisplayName(ShellDisplayName displayName, out IntPtr name);
        [PreserveSig] int GetAttributes(uint mask, out uint attributes);
        [PreserveSig] int Compare([MarshalAs(UnmanagedType.Interface)] IShellItem other, uint hint, out int order);
    }

    private enum ShellDisplayName : uint
    {
        FileSystemPath = 0x80058000
    }

    [Flags]
    private enum DesktopSlideshowOptions : uint
    {
        ShuffleImages = 0x1
    }

    private enum DesktopWallpaperPosition
    {
        Center,
        Tile,
        Stretch,
        Fit,
        Fill,
        Span
    }

    private enum DesktopSlideshowDirection
    {
        Forward,
        Backward
    }

    [Flags]
    private enum DesktopSlideshowState : uint
    {
        Enabled = 0x1,
        Slideshow = 0x2,
        DisabledByRemoteSession = 0x4
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }
}
