/* ###
 * IP: GHIDRA
 *
 * Script de Ghidra para el servidor MCP de Hikaru OpenLive.
 *
 * La cabecera con "###" NO es decorativa: el proveedor de scripts Java de
 * Ghidra la usa para armar el bundle de la clase. Sin ella, el script
 * compila pero la clase no se encuentra y el headless aborta con
 * ClassNotFoundException.
 *
 * OJO: este archivo tiene que estar en ASCII puro. El bloque de comentario
 * de arriba se cierra en la primera ocurrencia de fin de comentario, asi que
 * no se puede anidar otro dentro. Y los acentos rompen la compilacion si
 * javac no lee el archivo como UTF-8.
 *
 * Uso:  analyzeHeadless <proyecto> <nombre> -import <binario> \
 *           -scriptPath <dir> -postScript Decomp.java [modo] [objetivo]
 *
 * Modos:
 *   overview   (default) - resumen del programa: funciones, secciones, exports
 *   decompile            - pseudocode de UNA funcion (por nombre o direccion)
 *   functions            - el listado de funciones
 *   strings              - los strings del binario
 *
 * La salida se marca con lineas HIKARU_* para que el servidor la pueda
 * parsear sin depender del formato de los logs de Ghidra.
 *
 * @author Hikaru OpenStudio
 * @category Analysis
 * @keybinding
 * @menupath
 * @toolbar
 */

import java.util.ArrayList;
import java.util.List;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Symbol;
import ghidra.program.model.symbol.SymbolIterator;
import ghidra.program.model.symbol.SymbolType;

public class Decomp extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String modo = args.length > 0 ? args[0] : "overview";
        String objetivo = args.length > 1 ? args[1] : null;

        println("HIKARU_BEGIN");
        println("HIKARU_PROGRAMA=" + currentProgram.getName());
        println("HIKARU_FUNCIONES=" + currentProgram.getFunctionManager().getFunctionCount());
        println("HIKARU_MODO=" + modo);

        try {
            if ("decompile".equals(modo)) {
                decompilar(objetivo);
            } else if ("functions".equals(modo)) {
                listarFunciones();
            } else if ("strings".equals(modo)) {
                listarStrings();
            } else {
                overview();
            }
        } catch (Exception e) {
            println("HIKARU_ERROR=" + e.getClass().getSimpleName() + ": " + e.getMessage());
        }

        println("HIKARU_END");
    }

    /** El resumen general del binario. */
    private void overview() {
        println("HIKARU_ARCH=" + currentProgram.getLanguageID());
        println("HIKARU_COMPILADOR=" + currentProgram.getCompilerSpec().getCompilerSpecID());

        MemoryBlock[] bloques = currentProgram.getMemory().getBlocks();
        StringBuilder secciones = new StringBuilder();
        for (MemoryBlock bloque : bloques) {
            if (secciones.length() > 0) {
                secciones.append(",");
            }
            secciones
                .append(bloque.getName())
                .append(":")
                .append(bloque.getSize());
        }
        println("HIKARU_SECCIONES=" + secciones);

        // Los simbolos exportados son lo que delata el ABI del plugin.
        SymbolIterator simbolos = currentProgram.getSymbolTable().getAllSymbols(false);
        List<String> exports = new ArrayList<>();
        while (simbolos.hasNext() && exports.size() < 400) {
            Symbol simbolo = simbolos.next();
            if (simbolo.isExternalEntryPoint() || simbolo.getSymbolType() == SymbolType.FUNCTION) {
                String nombre = simbolo.getName();
                if (nombre.contains("Factory")
                    || nombre.contains("ModuleEntry")
                    || nombre.contains("BundleEntry")
                    || nombre.contains("clap")
                    || nombre.contains("main")) {
                    exports.add(nombre + "@" + simbolo.getAddress());
                }
            }
        }
        println("HIKARU_EXPORTS=" + String.join(",", exports));
    }

    /** El listado de funciones, con su direccion y su tamano. */
    private void listarFunciones() {
        FunctionIterator iterador =
            currentProgram.getFunctionManager().getFunctions(true);
        StringBuilder lote = new StringBuilder();
        int n = 0;
        int chunk = 0;
        while (iterador.hasNext() && n < 3000) {
            Function funcion = iterador.next();
            if (lote.length() > 0) {
                lote.append(",");
            }
            lote
                .append(funcion.getName())
                .append("@")
                .append(funcion.getEntryPoint())
                .append(":")
                .append(funcion.getBody().getNumAddresses());
            n++;

            // El log de Ghidra corta las lineas largas: se corta en trozos.
            // Cada trozo lleva un sufijo numerico para que no se pisen en el
            // parseo (si todos fueran "FUNCS", el último pisaría al primero).
            if (lote.length() > 1500) {
                println("HIKARU_FUNCS_" + chunk + "=" + lote);
                lote.setLength(0);
                chunk++;
            }
        }
        if (lote.length() > 0) {
            println("HIKARU_FUNCS_" + chunk + "=" + lote);
            chunk++;
        }
        println("HIKARU_FUNCS_CHUNKS=" + chunk);
        println("HIKARU_FUNCS_TOTAL=" + n);
    }

    /** Los strings del binario (con un tope: algunos plugins tienen miles). */
    private void listarStrings() {
        Listing listado = currentProgram.getListing();
        DataIterator datos =
            listado.getDefinedData(true);
        StringBuilder lista = new StringBuilder();
        int n = 0;
        while (datos.hasNext() && n < 500) {
            Object objeto = datos.next();
            if (objeto instanceof Data) {
                Data dato = (Data) objeto;
                if (dato.hasStringValue()) {
                    String valor = dato.getDefaultValueRepresentation();
                    if (valor != null && valor.length() > 2) {
                        if (lista.length() > 0) {
                            lista.append(",");
                        }
                        lista.append(valor.replace('\n', ' '));
                        n++;
                    }
                }
            }
        }
        println("HIKARU_STRING_LIST=" + lista);
    }

    /** El pseudocode de una funcion, por nombre o por direccion. */
    private void decompilar(String objetivo) throws Exception {
        if (objetivo == null || objetivo.isEmpty()) {
            println("HIKARU_ERROR=falta el nombre o la direccion de la funcion");
            return;
        }

        Function funcion = buscarFuncion(objetivo);
        if (funcion == null) {
            println("HIKARU_ERROR=no se encontro la funcion '" + objetivo + "'");
            return;
        }

        DecompInterface decompilador = new DecompInterface();
        decompilador.setOptions(new DecompileOptions());
        decompilador.openProgram(currentProgram);
        try {
            DecompileResults resultado = decompilador.decompileFunction(funcion, 120, monitor);
            if (!resultado.decompileCompleted()) {
                println("HIKARU_ERROR=" + resultado.getErrorMessage());
                return;
            }
            println("HIKARU_FUNC=" + funcion.getName());
            println("HIKARU_ADDR=" + funcion.getEntryPoint());
            println("HIKARU_SIG=" + funcion.getSignature());
            println("HIKARU_CALLERS=" + contarLlamadores(funcion));
            println("HIKARU_CODE_BEGIN");
            println(resultado.getDecompiledFunction().getC());
            println("HIKARU_CODE_END");
        } finally {
            decompilador.dispose();
        }
    }

    /** Busca la funcion por nombre exacto, por simbolo, por prefijo o por direccion. */
    private Function buscarFuncion(String objetivo) {
        // 1. Por direccion.
        try {
            Address direccion = currentProgram.getAddressFactory()
                .getAddress(objetivo);
            if (direccion != null) {
                println("HIKARU_LOOKUP=direccion " + direccion);
                Function porDireccion =
                    currentProgram.getFunctionManager().getFunctionAt(direccion);
                if (porDireccion != null) {
                    return porDireccion;
                }
                Function porRango =
                    currentProgram.getFunctionManager().getFunctionContaining(direccion);
                if (porRango != null) {
                    return porRango;
                }
            } else {
                println("HIKARU_LOOKUP=direccion nula");
            }
        } catch (Exception e) {
            // No es una direccion: sigue con el nombre.
            println("HIKARU_LOOKUP=direccion fallo " + e.getClass().getSimpleName());
        }

        // 2. Por la tabla de simbolos. En una biblioteca compartida el punto
        //    de entrada suele quedar como Label y no como Function: en ese
        //    caso hay que desensamblar y crear la funcion a mano.
        SymbolIterator simbolos = currentProgram.getSymbolTable().getSymbolIterator(
            objetivo, true);
        while (simbolos.hasNext()) {
            Symbol simbolo = simbolos.next();
            println("HIKARU_LOOKUP=simbolo " + simbolo.getName() + "@" + simbolo.getAddress()
                + " tipo=" + simbolo.getSymbolType());

            Function porSimbolo =
                currentProgram.getFunctionManager().getFunctionAt(simbolo.getAddress());
            if (porSimbolo != null) {
                return porSimbolo;
            }

            Function creada = crearFuncion(simbolo.getName(), simbolo.getAddress());
            if (creada != null) {
                return creada;
            }
        }

        // 3. Por nombre exacto.
        FunctionIterator iterador =
            currentProgram.getFunctionManager().getFunctions(true);
        while (iterador.hasNext()) {
            Function funcion = iterador.next();
            if (funcion.getName().equals(objetivo)) {
                return funcion;
            }
        }

        // 4. Por prefijo (los nombres de C++ vienen con la firma completa).
        iterador = currentProgram.getFunctionManager().getFunctions(true);
        while (iterador.hasNext()) {
            Function funcion = iterador.next();
            if (funcion.getName().startsWith(objetivo + "(")
                || funcion.getName().startsWith(objetivo + "::")) {
                return funcion;
            }
        }

        // 5. Una muestra de los nombres que hay, para que el error diga algo.
        iterador = currentProgram.getFunctionManager().getFunctions(true);
        StringBuilder muestra = new StringBuilder();
        int n = 0;
        while (iterador.hasNext() && n < 10) {
            Function funcion = iterador.next();
            if (muestra.length() > 0) {
                muestra.append(",");
            }
            muestra.append(funcion.getName());
            n++;
        }
        println("HIKARU_MUESTRA=" + muestra);

        return null;
    }

    /**
     * Crea la funcion en una direccion donde solo hay un Label.
     *
     * En los binarios de plugin (bibliotecas compartidas) el analizador de
     * Ghidra no siempre convierte el punto de entrada en una funcion: lo deja
     * como etiqueta. Sin desensamblar y crear la funcion, el decompilador no
     * tiene nada que decompilar.
     */
    private Function crearFuncion(String nombre, Address direccion) {
        try {
            disassemble(direccion);
            Function creada = createFunction(direccion, nombre);
            if (creada != null) {
                println("HIKARU_CREADA=" + creada.getName() + "@" + creada.getEntryPoint());
            }
            return creada;
        } catch (Exception e) {
            println("HIKARU_ERROR_CREAR=" + e.getClass().getSimpleName() + ": " + e.getMessage());
            return null;
        }
    }

    /** Cuantas funciones la llaman: sirve para saber si es el punto de entrada. */
    private String contarLlamadores(Function funcion) {
        int n = 0;
        java.util.Collection<Function> llamadores = funcion.getCallingFunctions(monitor);
        if (llamadores != null) {
            n = llamadores.size();
        }
        return String.valueOf(n);
    }
}
