#![allow(non_snake_case)]
use std::{os::windows::ffi::OsStrExt, path::PathBuf};
use windows::{
    Win32::{Foundation::E_ABORT, System::Com::*, UI::Shell::*},
    core::{HRESULT, PCWSTR, Ref, Result, implement},
};

// Refuse the Shell's permanent-delete path, including a full or disabled bin.
#[implement(IFileOperationProgressSink)]
struct RecycleOnly;
impl IFileOperationProgressSink_Impl for RecycleOnly_Impl {
    fn StartOperations(&self) -> Result<()> {
        Ok(())
    }
    fn FinishOperations(&self, result: HRESULT) -> Result<()> {
        result.ok()
    }
    fn PreDeleteItem(&self, flags: u32, _: Ref<'_, IShellItem>) -> Result<()> {
        if flags & TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32 == 0 {
            Err(E_ABORT.into())
        } else {
            Ok(())
        }
    }
    fn PostDeleteItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        result: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        result.ok()
    }
    fn PreRenameItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> Result<()> {
        Err(E_ABORT.into())
    }
    fn PostRenameItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Err(E_ABORT.into())
    }
    fn PostMoveItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
    ) -> Result<()> {
        Err(E_ABORT.into())
    }
    fn PostCopyItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn PreNewItem(&self, _: u32, _: Ref<'_, IShellItem>, _: &PCWSTR) -> Result<()> {
        Err(E_ABORT.into())
    }
    fn PostNewItem(
        &self,
        _: u32,
        _: Ref<'_, IShellItem>,
        _: &PCWSTR,
        _: &PCWSTR,
        _: u32,
        _: HRESULT,
        _: Ref<'_, IShellItem>,
    ) -> Result<()> {
        Ok(())
    }
    fn UpdateProgress(&self, _: u32, _: u32) -> Result<()> {
        Ok(())
    }
    fn ResetTimer(&self) -> Result<()> {
        Ok(())
    }
    fn PauseTimer(&self) -> Result<()> {
        Ok(())
    }
    fn ResumeTimer(&self) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn recycle(paths: &[PathBuf]) -> anyhow::Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }
    let _com = Com;
    unsafe {
        let operation: IFileOperation =
            CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
        operation.SetOperationFlags(
            FOFX_RECYCLEONDELETE
                | FOFX_EARLYFAILURE
                | FOF_SILENT
                | FOF_NOERRORUI
                | FOF_NOCONFIRMATION
                | FOF_WANTNUKEWARNING,
        )?;
        let sink: IFileOperationProgressSink = RecycleOnly.into();
        for path in paths {
            let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
            operation.DeleteItem(&item, &sink)?;
        }
        operation.PerformOperations()?;
        anyhow::ensure!(
            !operation.GetAnyOperationsAborted()?.as_bool(),
            "recycle unavailable"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permanent_delete_is_refused_by_the_shell_sink() {
        let sink: IFileOperationProgressSink = RecycleOnly.into();
        unsafe {
            assert!(sink.PreDeleteItem(0, None).is_err());
            assert!(
                sink.PreDeleteItem(TSF_DELETE_RECYCLE_IF_POSSIBLE.0 as u32, None)
                    .is_ok()
            );
        }
    }
}
