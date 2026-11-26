---
id: exporting
title: Exportação
group: publishing
---

# Exportação

Uma exportação é um pacote pronto para postar: para cada rede, o arquivo renderizado e um arquivo de texto com tudo o que vai no formulário de upload da rede. É assim que você posta na Kick, para onde o Bardo não envia, e funciona para todas as redes. As exportações começam na etapa [Publicação](bardo:go/projects/publish).

<a id="choose"></a>
## Escolher as redes

A linha de cada rede tem uma caixa: marque as redes a exportar. Redes ainda não exportadas, ou cuja última exportação está desatualizada, já vêm marcadas. Uma rede pode ser exportada quando tem um arquivo renderizado e os metadados escritos e dentro dos limites da rede ([Metadados e seus limites](uploading.md#metadata)); até lá, a linha dela diz o que ainda falta, e salvar ou reverter edições não salvas vem antes.

<a id="export"></a>
## Exportar

**Exportar (n)** roda uma tarefa para as redes escolhidas, com o progresso na etapa e em Tarefas, para você continuar trabalhando. **Cancelar** para a tarefa; **Retomar** exporta as redes que faltam e mantém as prontas. Uma exportação só começa depois que o render do projeto termina, porque o render reescreve os arquivos que ela copia.

<a id="folder"></a>
## A pasta

A exportação de cada projeto tem uma pasta, com uma pasta por rede dentro, com o nome da rede. Cada uma tem:

- o vídeo renderizado, no preset da rede;
- o `metadata.txt`, no idioma da interface: o título, a descrição ou legenda e as tags para colar em cada campo, a visibilidade a escolher e, quando a voz é sintética, onde ativar o rótulo da rede; num anúncio, a divulgação comercial e o que ativar ao publicar, e os itens que você confirmou antes de exportar ([Antes de um anúncio ir ao ar](uploading.md#ad-review));
- na pasta do YouTube, o `thumbnail.jpg` quando o projeto tem uma [miniatura](uploading.md#thumbnail) salva, para aplicar no YouTube Studio.

**Mostrar na pasta** abre a pasta, e **Detalhes** mostra o caminho dela. O inspetor também tem **Copiar** ao lado de cada campo, para colar direto do Bardo.

<a id="outdated"></a>
## Desatualizada

A última exportação de uma rede aparece como **Exportada** enquanto bate com o render, os metadados e a miniatura do YouTube, **Desatualizada** quando um deles mudou depois dela e **Não exportada** antes da primeira. Exporte de novo para atualizar a pasta. Um arquivo com render desatualizado exporta como está; renderize de novo na etapa [Render](render.md) para incluir as mudanças.

<a id="ads"></a>
## Exportar para anúncios

Um anúncio tem uma segunda exportação na etapa Publicação, **Exportar para anúncios**, para os gerenciadores de anúncios onde você o veicula como anúncio pago. O Bardo nunca envia nada a um gerenciador de anúncios: você mesmo sobe os arquivos.

- **Formatos.** Ela renderiza o corte em 4:5 (1080 × 1350) e 1:1 (1080 × 1080), os formatos que os gerenciadores de anúncios pedem, e no outro formato do editor (16:9 para um corte vertical, 9:16 para um horizontal). Um formato ainda não exportado, ou desatualizado, já vem marcado.
- **Imagens compostas.** As fotos do produto e o cartão final são compostos de novo para cada formato, então o produto, o texto dele e o botão ficam inteiros.
- **Janelas.** Cada outra imagem e cada clipe preenche o quadro por uma janela, centralizada a menos que você a mova. **Janela por clipe** mostra a janela de cada clipe no formato escolhido; as setas a movem e **Centralizar** a traz de volta. Cada clipe guarda uma janela por formato. Em 16:9 cada imagem aparece inteira, com faixas onde não alcança o quadro.
- **Legendas** são diagramadas de novo para o formato, e nenhuma aparece sobre o cartão final.
- **Textos para o gerenciador de anúncios.** O job que escreve os textos dos posts também escreve um texto principal (até 125 caracteres, acima do vídeo) e um título (até 40, ao lado do botão), uma vez para todos os formatos, e a [conferência dos textos dos posts](uploading.md#ad-review) também os lê. Edite-os no cartão e escolha **Salvar textos**; a exportação espera enquanto um deles estiver vazio ou longo demais.

A exportação espera pelas cenas do produto em uso que você ainda não conferiu e por uma imagem composta que precise ser feita de novo, e vai com os itens que você confirmou na revisão do anúncio. **Exportar para anúncios** roda um job, com o progresso na etapa e em Jobs; **Cancelar** o interrompe, e exportar de novo termina os formatos que faltam.

Cada formato ganha uma pasta na pasta de exportação do projeto, `Ads/4x5`, `Ads/1x1` e `Ads/16x9` ou `Ads/9x16`, com o vídeo e o `ad.txt`: o arquivo, o tamanho e o formato dele, o texto principal, o título, a chamada para ação e o link do briefing, a divulgação comercial a marcar, o rótulo de conteúdo sintético quando o vídeo precisa dele ([Conteúdo sintético](uploading.md#disclosure)), e os itens que você confirmou. **Mostrar na pasta** a abre. Um formato aparece como **Exportado**, **Desatualizado** quando o corte, uma imagem composta ou os textos mudaram depois dele, ou **Não exportado**.

<a id="post"></a>
## Depois de postar à mão

Com o post no ar, cole o link dele na seção **Publicação** da rede e escolha **Marcar como publicado**, para o Bardo acompanhar os números dele em [Desempenho](bardo:go/performance). Veja [A publicação](uploading.md#post).
