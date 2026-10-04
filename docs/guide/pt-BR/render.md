---
id: render
title: Revisão e render
group: editing
place: projects/render
tour: render
---

# Revisão e render

O render transforma o corte nos arquivos que você publica: um por conta de rede do canal, no preset da rede. Ele leva um tempo e substitui o último arquivo de cada rede, então a etapa [Render](bardo:go/projects/render) primeiro mostra o que vai sair e o que está no caminho, e só renderiza depois que você confirma. [Mostre a etapa para mim](bardo:tour/render).

<a id="review"></a>
## A revisão

Abrir a etapa verifica o corte e este computador: mede a mixagem e testa os codificadores de vídeo, o que leva alguns segundos. Os números no topo descrevem o corte como ele vai sair:

| Número | Diz |
| --- | --- |
| Duração | Quanto tempo o corte dura |
| Quadro do corte | 16:9 ou 9:16, como definido [no editor](editor.md#framing) |
| Loudness da mixagem | O loudness integrado da mixagem, em LUFS |
| Legendas | Se as legendas aparecem |

Um número em âmbar merece uma olhada: uma mixagem sem som, ou legendas desligadas. **Verificar de novo** roda as verificações outra vez, depois que você muda algo fora do corte.

<a id="targets"></a>
## Redes e presets

Cada conta de rede do canal é um destino, listado com o @, o preset, a situação e o último arquivo. Cada rede parte do próprio preset:

| Rede | Quadro | Tamanho | Bitrate | Duração máxima |
| --- | --- | --- | --- | --- |
| YouTube | 9:16 | 1080×1920 | 12 Mbps | 3 min |
| TikTok | 9:16 | 1080×1920 | 10 Mbps | 10 min |
| Instagram Reels | 9:16 | 1080×1920 | 10 Mbps | 15 min |
| X | 9:16 | 720×1280 | 6 Mbps | 2 min 20 s |
| Kick | 16:9 | 1920×1080 | 8 Mbps | 12 h |

Todas usam H.264 e miram −14 LUFS. Dá para mudar o preset de uma conta (quadro, tamanho, codec, bitrate, duração máxima e loudness) em [Contas](bardo:go/accounts). Um canal sem contas não tem para o que renderizar: adicione uma lá primeiro.

Marque as redes que vão renderizar; as que ainda não têm arquivo, ou estão desatualizadas, já vêm marcadas. Clique numa rede para vê-la no inspetor: o preset, o codificador que este computador vai usar (na placa de vídeo quando dá), as verificações e o último arquivo.

<a id="gates"></a>
## O que bloqueia e o que avisa

Cada verificação vem marcada **Bloqueia** ou **Aviso**.

- **Bloqueia** deixa a rede fora do render até você corrigir: um corte mais longo do que a rede aceita, ou nenhum codificador neste computador para o codec do preset. As outras redes renderizam mesmo assim.
- **Aviso** deixa renderizar como está: legendas desligadas, clipes sem mídia (saem pretos), mixagem sem som, mixagem longe do loudness da rede (o render ajusta essa diferença), ou picos que o render precisa limitar. Quando o quadro do corte difere do da rede, cada clipe passa pela janela de recorte.

A situação de cada rede resume tudo: **Pronta**, **n para conferir** (avisos), **Bloqueada**, ou **Verificando** enquanto as verificações rodam.

<a id="render"></a>
## Renderizar

**Renderizar (n)** pergunta mais uma vez: quantos arquivos, que cada um substitui o último da rede, e quantos avisos ficam. **Renderizar agora** coloca um único job na fila para todos. O job roda em segundo plano, com o progresso na etapa e em Tarefas, então você pode continuar trabalhando; **Cancelar** para, e **Retomar** renderiza os arquivos que faltam e mantém os prontos. O render espera enquanto uma exportação está copiando os arquivos.

<a id="last"></a>
## O último arquivo

Cada rede guarda o último arquivo renderizado. **Renderizado** quer dizer que ele bate com o corte e o preset de agora; **Desatualizado** quer dizer que o corte ou o preset mudou depois, então o próximo render o marca de novo; **Não renderizado** quer dizer que ainda não há nenhum. O inspetor mostra o tamanho, o loudness (e se acertou o alvo) e o codificador, e **Mostrar na pasta** abre o arquivo. A etapa Publicação exporta e envia esses arquivos.
