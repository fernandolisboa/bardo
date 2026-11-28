---
id: render
title: Revisão e render
group: editing
place: videos/render
tour: render
---

# Revisão e render

O render transforma o corte nos arquivos que você publica ou compartilha: um por rede marcada, no preset da rede. Um arquivo nunca precisa de conta; só o [envio](uploading.md) precisa. Ele leva um tempo e substitui o último arquivo de cada rede, então a etapa [Render](bardo:go/videos/render) primeiro mostra o que vai sair e o que está no caminho, e só renderiza depois que você confirma. [Mostre a etapa para mim](bardo:tour/render).

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

Toda rede é um destino, listado com o @, o preset, a situação e o último arquivo. Uma rede em que o canal tem [conta](bardo:go/accounts) mostra o @ dessa conta. Todas as outras, e todas as redes de um [vídeo avulso](projects.md#loose-videos), mostram **Arquivo**: o Bardo renderiza o arquivo para você baixar, mandar para alguém ou publicar por conta própria, e ele nunca é enviado pelo Bardo. Cada rede parte do próprio preset, e o do YouTube segue o [formato](projects.md#format) do projeto:

| Rede | Quadro | Tamanho | Bitrate | Duração máxima |
| --- | --- | --- | --- | --- |
| YouTube, Short | 9:16 | 1080×1920 | 12 Mbps | 3 min |
| YouTube, vídeo longo | 16:9 | 1920×1080 | 12 Mbps | 12 h |
| TikTok | 9:16 | 1080×1920 | 10 Mbps | 10 min |
| Instagram Reels | 9:16 | 1080×1920 | 10 Mbps | 15 min |
| X | 9:16 | 720×1280 | 6 Mbps | 2 min 20 s |
| Kick | 16:9 | 1920×1080 | 8 Mbps | 12 h |

Todas usam H.264 e miram −14 LUFS. O preset de uma conta (quadro, tamanho, codec, bitrate, duração máxima e loudness) muda em [Contas](bardo:go/accounts), para todos os vídeos do canal; os [ajustes deste vídeo](#settings) mudam só para um vídeo.

Marque as redes que vão renderizar. As contas que ainda não têm arquivo, ou estão desatualizadas, já vêm marcadas, e um arquivo desatualizado também; os outros arquivos não, para o render ficar nas redes para as quais o canal existe. Um vídeo sem nenhuma conta, e ainda sem nada renderizado, começa com um arquivo marcado: a primeira rede no quadro do próprio corte. Clique numa rede para vê-la no inspetor: o preset, o codificador que este computador vai usar (na placa de vídeo quando dá), as verificações, os ajustes deste vídeo e o último arquivo.

<a id="settings"></a>
## Ajustes deste vídeo

Os **Ajustes deste vídeo** no inspetor mudam o quadro, o tamanho, o codec, o bitrate, a duração máxima e o loudness da rede escolhida só para este vídeo: por exemplo um arquivo 16:9 em 720p para mandar para um amigo, ou um limite maior para um arquivo que você vai publicar à mão. Cada um começa em **Padrão**, o valor da conta (ou da rede, para um arquivo); campos em branco mantêm esse valor. **Salvar ajustes** guarda os ajustes, e o último arquivo da rede fica desatualizado até você renderizar de novo. **Restaurar padrões** volta ao preset da conta ou da rede. O preset da própria conta nunca muda aqui.

<a id="gates"></a>
## O que bloqueia e o que avisa

Cada verificação vem marcada **Bloqueia** ou **Aviso**.

- **Bloqueia** deixa a rede fora do render até você corrigir: um corte mais longo do que a rede aceita, ou nenhum codificador neste computador para o codec do preset. Os dois mudam nos [ajustes deste vídeo](#settings). As outras redes renderizam mesmo assim.
- Num anúncio, uma cena do [produto em uso](scenes.md#product-check) cujo produto você não conferiu impede o vídeo inteiro de renderizar: a revisão lista essas cenas. Confira cada uma em Cenas, ou use ali a imagem real.
- Num anúncio, um [cartão final](scenes.md#end-card) ainda não composto com o design, a oferta do briefing e o logo atuais impede o vídeo inteiro de renderizar. Componha em Cenas: é grátis.
- **Aviso** deixa renderizar como está: legendas desligadas, clipes sem mídia (saem pretos), mixagem sem som, mixagem longe do loudness da rede (o render ajusta essa diferença), picos que o render precisa limitar, ou um vídeo de YouTube com mais de 15 minutos, que o YouTube só aceita de uma [conta verificada](https://www.youtube.com/verify). Quando o quadro do corte difere do da rede, um arquivo 9:16 passa cada clipe pela janela de recorte, e um arquivo 16:9 mostra cada imagem inteira, uma em pé com faixas nas laterais.

A situação de cada rede resume tudo: **Pronta**, **n para conferir** (avisos), **Bloqueada**, ou **Verificando** enquanto as verificações rodam.

<a id="render"></a>
## Renderizar

**Renderizar (n)** pergunta mais uma vez: quantos arquivos, que cada um substitui o último da rede, e quantos avisos ficam. **Renderizar agora** coloca um único job na fila para todos. O job roda em segundo plano, com o progresso na etapa e em Tarefas, então você pode continuar trabalhando; **Cancelar** para, e **Retomar** renderiza os arquivos que faltam e mantém os prontos. O render espera enquanto uma exportação está copiando os arquivos.

<a id="last"></a>
## O último arquivo

Cada rede guarda o último arquivo renderizado. **Renderizado** quer dizer que ele bate com o corte e o preset de agora; **Desatualizado** quer dizer que o corte ou o preset mudou depois, então o próximo render o marca de novo; **Não renderizado** quer dizer que ainda não há nenhum. O inspetor mostra o tamanho, o loudness (e se acertou o alvo) e o codificador, e **Mostrar na pasta** abre o arquivo. A etapa Publicação exporta esses arquivos, e envia os que têm uma conta por trás.
