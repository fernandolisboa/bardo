---
id: exporting
title: Exportação
group: publishing
---

# Exportação

Uma exportação é um pacote pronto para postar: para cada rede, o arquivo renderizado e um arquivo de texto com tudo o que vai no formulário de upload da rede. É assim que você posta no X e na Kick, para onde o Bardo não envia, e funciona para todas as redes. As exportações começam na etapa [Publicação](bardo:go/projects/publish).

<a id="choose"></a>
## Escolher as redes

A linha de cada rede tem uma caixa: marque as redes a exportar. Redes ainda não exportadas, ou cuja última exportação está desatualizada, já vêm marcadas. Uma rede pode ser exportada quando tem um arquivo renderizado e os metadados escritos e dentro dos limites da rede ([Metadados e seus limites](uploading.md#metadata)); até lá, a linha dela diz o que ainda falta, e salvar ou reverter edições não salvas vem antes.

<a id="export"></a>
## Exportar

**Exportar (n)** roda uma tarefa para as redes escolhidas, com o progresso na etapa e em Tarefas, para você continuar trabalhando. **Cancelar** para a tarefa; **Retomar** exporta as redes que faltam e mantém as prontas. Uma exportação espera enquanto o projeto renderiza, porque o render reescreve os arquivos que ela copia.

<a id="folder"></a>
## A pasta

A exportação de cada projeto tem uma pasta, com uma pasta por rede dentro, com o nome da rede. Cada uma tem:

- o vídeo renderizado, no preset da rede;
- o `metadata.txt`, no idioma da interface: o título, a descrição ou legenda e as tags para colar em cada campo, a visibilidade a escolher e, quando a voz é sintética, onde ativar o rótulo da rede.

**Mostrar na pasta** abre a pasta, e **Detalhes** mostra o caminho dela. O inspetor também tem **Copiar** ao lado de cada campo, para colar direto do Bardo.

<a id="outdated"></a>
## Desatualizada

A última exportação de uma rede aparece como **Exportada** enquanto bate com o render e os metadados, **Desatualizada** quando um dos dois mudou depois dela e **Não exportada** antes da primeira. Exporte de novo para atualizar a pasta. Um arquivo com render desatualizado exporta como está; renderize de novo na etapa [Render](render.md) para incluir as mudanças.

<a id="post"></a>
## Depois de postar à mão

Com o post no ar, cole o link dele na seção **Publicação** da rede e escolha **Marcar como publicado**, para o Bardo acompanhar os números dele em [Desempenho](bardo:go/performance). Veja [A publicação](uploading.md#post).
