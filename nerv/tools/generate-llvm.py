from pathlib import Path


FUNCTIONS = """
ContextCreate > p
ContextDispose p > v
ModuleCreateWithNameInContext s p > p
DisposeModule p > v
CreateBuilderInContext p > p
DisposeBuilder p > v
VoidTypeInContext p > p
IntTypeInContext p u > p
FloatTypeInContext p > p
DoubleTypeInContext p > p
PointerTypeInContext p u > p
StructCreateNamed p s > p
StructSetBody p p u b > v
StructTypeInContext p p u b > p
FunctionType p p u b > p
AddFunction p s p > p
GetParam p u > p
GetIntTypeWidth p > b
GetEntryBasicBlock p > p
GetFirstInstruction p > p
PositionBuilderBefore p p > v
AppendBasicBlockInContext p p s > p
PositionBuilderAtEnd p p > v
GetInsertBlock p > p
GetBasicBlockParent p > p
GetBasicBlockTerminator p > p
BuildAlloca p p s > p
BuildStore p p p > p
BuildLoad2 p p p s > p
BuildRet p p > p
BuildRetVoid p > p
BuildBr p p > p
BuildCondBr p p p p > p
BuildUnreachable p > p
BuildAdd p p p s > p
BuildSub p p p s > p
BuildMul p p p s > p
BuildSDiv p p p s > p
BuildUDiv p p p s > p
BuildSRem p p p s > p
BuildURem p p p s > p
BuildFAdd p p p s > p
BuildFSub p p p s > p
BuildFMul p p p s > p
BuildFDiv p p p s > p
BuildFRem p p p s > p
BuildAnd p p p s > p
BuildOr p p p s > p
BuildNeg p p s > p
BuildFNeg p p s > p
BuildNot p p s > p
BuildICmp p b p p s > p
BuildFCmp p b p p s > p
BuildCall2 p p p p u s > p
BuildExtractValue p p u s > p
BuildInsertValue p p p u s > p
BuildGEP2 p p p p u s > p
BuildStructGEP2 p p p u s > p
BuildPhi p p s > p
AddIncoming p p p u > v
BuildIntCast2 p p p b s > p
BuildSIToFP p p p s > p
BuildUIToFP p p p s > p
BuildFPToSI p p p s > p
BuildFPToUI p p p s > p
BuildFPCast p p p s > p
BuildIntToPtr p p p s > p
BuildPtrToInt p p p s > p
ConstInt p l b > p
ConstIntOfStringAndSize p s u u > p
ConstReal p d > p
ConstNull p > p
GetUndef p > p
SizeOf p > p
BuildGlobalStringPtr p s s > p
ConstStringInContext2 p p l b > p
AddGlobal p p s > p
SetInitializer p p > v
SetGlobalConstant p b > v
TypeOf p > p
VerifyModule p b p > b
PrintModuleToString p > p
DisposeMessage p > v
GetDefaultTargetTriple > p
SetTarget p s > v
InitializeX86TargetInfo > v
InitializeX86Target > v
InitializeX86TargetMC > v
InitializeX86AsmPrinter > v
InitializeAArch64TargetInfo > v
InitializeAArch64Target > v
InitializeAArch64TargetMC > v
InitializeAArch64AsmPrinter > v
GetTargetFromTriple s p p > b
CreateTargetMachine p s s s b b b > p
DisposeTargetMachine p > v
CreateTargetDataLayout p > p
CopyStringRepOfTargetData p > p
SetDataLayout p s > v
DisposeTargetData p > v
TargetMachineEmitToFile p p s b p > b
CreatePassBuilderOptions > p
DisposePassBuilderOptions p > v
RunPasses p s p p > p
GetErrorMessage p > p
DisposeErrorMessage p > v
"""


def main():
    functions = [line.split() for line in FUNCTIONS.splitlines() if line.strip()]
    lines = ['import nerv.std', '', 'struct LLVM', '    library: i64', '    functions: []i64', '', 'fn open_library(file: string) -> LLVM', '    let library = nerv.ffi.open(file)', f'    let mut functions = [0] * {len(functions)}', '    if library != 0']
    for index, tokens in enumerate(functions):
        lines.append(f'        functions[{index}] = nerv.ffi.symbol(library, "LLVM{tokens[0]}")')
    lines += ['    LLVM { library: library, functions: functions }', '']
    for index, tokens in enumerate(functions):
        name = tokens[0]
        parameters = tokens[1:tokens.index('>')]
        result = tokens[-1]
        types = {'s': 'string', 'd': 'f64'}
        declarations = [f'a{offset}: {types.get(kind, "i64")}' for offset, kind in enumerate(parameters)]
        arguments = [f'a{offset}' for offset in range(len(parameters))]
        temporaries = []
        for offset, kind in enumerate(parameters):
            if kind == 's':
                temporaries += [f'    let s{offset} = nerv.ffi.cstring(a{offset})']
                arguments[offset] = f's{offset}'
            if kind == 'u':
                arguments[offset] = f'u32(a{offset})'
            if kind == 'b':
                arguments[offset] = f'i32(a{offset})'
        return_type = {'v': 'void', 'd': 'f64'}.get(result, 'i64')
        call_type = {'v': 'void', 'b': 'i32', 'd': 'f64'}.get(result, 'i64')
        lines += [f'fn {name}(api: LLVM{", " if declarations else ""}{", ".join(declarations)}) -> {return_type}'] + temporaries
        lines += [f'    assert(api.functions[{index}] != 0)']
        call = f'nerv.ffi.call_{call_type}(api.functions[{index}]{", " if arguments else ""}{", ".join(arguments)})'
        lines += [f'    let result = {call}' if result != 'v' else f'    {call}']
        lines += [f'    nerv.ffi.free(s{offset})' for offset, kind in enumerate(parameters) if kind == 's']
        if result != 'v':
            lines += ['    i64(result)' if result == 'b' else '    result']
        lines += ['']
    Path(__file__).resolve().parent.parent.joinpath('src/llvm.nerv').write_text('\n'.join(lines), encoding='utf-8')


if __name__ == '__main__':
    main()
